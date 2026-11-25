//! Claude Code：`claude -p` 的双向 stream-json。用站主登录的订阅，不加 `--bare`（照平时读 CLAUDE.md、
//! hooks、MCP）。
//!
//! 一个回合：起进程 → 写一行 `user` → 读到 `result` 为止。审批用 `--permission-prompt-tool stdio`：
//! Claude Code 发 `control_request`（`can_use_tool`），Peck 回 `control_response`；`AskUserQuestion`
//! 也走这条路，回答放进 `updatedInput.answers`。追加一句就再写一行 `user`：Claude Code 在两步之间把它
//! 并进这一回合（只回一个 `result`），或者这一回合说完了再开一回合。带 `--replay-user-messages`，它吃进
//! 一句就回显一行 `isReplay` 的 `user`：写进去的都回显过、又到了 `result` 才算完；不回显的版本
//! `result` 之后 15 秒没动静也算完（2.1.322 真跑核实）。打断发 `control_request`（`interrupt`）。
//!
//! 权限三档：PLAN = `plan`，EDIT = `acceptEdits`，AUTO 也是 `acceptEdits`（放宽靠 Peck 自动批，
//! 高危照样弹卡片）。

use super::{emit_all, emit_engine_died, EngineSetup, Launch, Line, Pipe};
use crate::gateway::caps::AgentCaps;
use crate::gateway::convert::claude_code::ClaudeCodeConverter;
use crate::gateway::convert::LineConverter;
use crate::gateway::event::{EventBody, NoticeLevel, PermissionLevel, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, BoxFuture, Control, GatewayError, RunOutcome};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct ClaudeCodeBackend {
    setup: EngineSetup,
}

/// 本机 MCP 一次调用最多等多久（委派的子任务最多 24 轮）。
const PECK_MCP_TIMEOUT_MS: u64 = 30 * 60 * 1000;

impl ClaudeCodeBackend {
    pub fn new(setup: EngineSetup) -> Self {
        Self { setup }
    }

    pub fn args(task: &AgentTask) -> Vec<String> {
        let mode = match task.permission {
            PermissionLevel::Plan => "plan",
            PermissionLevel::Edit | PermissionLevel::Auto => "acceptEdits",
        };
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--replay-user-messages",
            "--permission-prompt-tool",
            "stdio",
            "--permission-mode",
            mode,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(model) = &task.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if let Some(effort) = &task.effort {
            args.extend(["--effort".into(), effort.clone()]);
        }
        if let Some(session) = &task.resume {
            args.extend(["--resume".into(), session.clone()]);
        }
        // 站主批过这个项目（v1.1.8）：`.mcp.json` 里的服务器这一次都连。
        if task.trusted {
            args.extend(crate::gateway::trust::launch_args("claude-code", &task.cwd));
        }
        // 项目里能改的其他文件夹（v0.10）：只这一次进程挂上（只读的只在这一句后面写明，读的时候它照常问）。
        for dir in super::writable_folders(task) {
            args.extend(["--add-dir".into(), dir]);
        }
        // Peck 的本机 MCP（v0.9 B1 委派时才给；v0.10 起每个任务都给，有 `create_skill`）：只这一次进程带上，不改 Claude Code 的配置；令牌走环境变量
        // （`${PECK_MCP_TOKEN}`，Claude Code 读这份配置时展开），不进命令行。`timeout` 一次放宽请求、空闲、工具
        // 三个超时（子任务要跑几分钟）；`alwaysLoad` 等它连上、第一轮就有这两个工具。
        // Peck 自己的两个工具不再问一遍（子任务里每一步照 Peck 的档位问）。
        if let Some(m) = &task.peck_mcp {
            let config = serde_json::json!({"mcpServers": {"peck": {
                "type": "http",
                "url": m.url,
                "headers": {"Authorization": "Bearer ${PECK_MCP_TOKEN}"},
                "timeout": PECK_MCP_TIMEOUT_MS,
                "alwaysLoad": true,
            }}});
            args.extend(["--allowedTools".into(), "mcp__peck__ask_user,mcp__peck__create_skill".into(), format!("--mcp-config={config}")]);
        }
        args
    }

    /// 起进程带的环境变量：项目的、不自动更新、本机 MCP 的令牌；还有清单工具——`claude -p` 默认不给
    /// `TaskCreate` / `TaskUpdate`，官方的 `CLAUDE_CODE_ENABLE_TODO_TOOLS` 打开（站主 2026-11-25 定，只对这一次进程，
    /// 不改它的配置；2.1.322 核实）。
    fn launch_env(task: &AgentTask, setup: &[(String, String)]) -> Vec<(String, String)> {
        let mut env = super::project_env(task);
        env.push(("DISABLE_AUTOUPDATER".to_string(), "1".to_string()));
        env.push(("CLAUDE_CODE_ENABLE_TODO_TOOLS".to_string(), "1".to_string()));
        if let Some(m) = &task.peck_mcp {
            env.push(("PECK_MCP_TOKEN".into(), m.token.clone()));
        }
        env.extend(setup.iter().cloned());
        env
    }
}

impl AgentBackend for ClaudeCodeBackend {
    fn setup(&self) -> Option<&EngineSetup> {
        Some(&self.setup)
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::claude_code()
    }

    fn run(
        &self,
        task: AgentTask,
        sink: EventSink,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let launch = Launch {
            bin: self.setup.bin.clone(),
            args: Self::args(&task),
            cwd: task.cwd.clone(),
            env: Self::launch_env(&task, &self.setup.env),
            path: self.setup.path.clone(),
        };
        Box::pin(async move {
            let pipe = Pipe::spawn(&launch)?;
            Session::new(pipe, &task, sink).run(task, control).await
        })
    }
}

pub(crate) struct Session {
    pipe: Pipe,
    sink: EventSink,
    converter: ClaudeCodeConverter,
    /// 等站主回答的审批和提问：request id → (工具名, 它要用的参数)。
    pending: HashMap<String, (String, Value)>,
    session_id: Option<String>,
    /// 写进去的 `user` 行数和收到的 `result` 数。
    sent_messages: usize,
    results: usize,
    /// 写进去、还没回显（`isReplay`）的话：空了说明它都吃进去了。
    unconsumed: VecDeque<String>,
    /// 最近一个 `result` 的时候，和那之后主线有没有又动起来（回显、说话、要审批）。
    last_result_at: Option<tokio::time::Instant>,
    active_since_result: bool,
    last_turn: Option<(TurnEndReason, String)>,
    interrupts: u32,
    /// 在后台跑的子代理（v0.9：`system` / `background_tasks_changed` 里 `local_agent` 的个数）。主线说完了还有，
    /// 不关进程，等它们做完、Claude Code 把主线叫醒再说完那一轮。
    background: usize,
    /// 后台子代理刚做完时的 `result` 数：再来一个才算叫醒的那一轮说完了。
    woken_at: Option<(usize, tokio::time::Instant)>,
    /// 主线说完、开始等后台的时候（最多等 [`BACKGROUND_WAIT`]）。
    waiting_since: Option<tokio::time::Instant>,
    /// 转发 `/compact`（v1.1.7）：开场时看它的斜杠命令里有没有 `compact`。
    compacting: bool,
}

/// 主线说完以后最多等后台子代理多久。
const BACKGROUND_WAIT: Duration = Duration::from_secs(30 * 60);
/// 后台做完以后等 Claude Code 叫醒主线、说完那一轮最多多久。
const WAKE_WAIT: Duration = Duration::from_secs(120);
/// 追加的话没见回显：`result` 之后这么久主线没再动，就当它已经并进那一回合了。
const QUEUE_WAIT: Duration = Duration::from_secs(15);

impl Session {
    pub(crate) fn new(pipe: Pipe, task: &AgentTask, sink: EventSink) -> Self {
        Self {
            pipe,
            sink,
            converter: ClaudeCodeConverter::new(Some(task.cwd.to_string_lossy().into_owned())),
            pending: HashMap::new(),
            session_id: None,
            sent_messages: 0,
            results: 0,
            unconsumed: VecDeque::new(),
            last_result_at: None,
            active_since_result: false,
            last_turn: None,
            interrupts: 0,
            background: 0,
            woken_at: None,
            waiting_since: None,
            compacting: task.compact,
        }
    }

    /// 到了 `result`、之后主线没再动，而且写进去的话都有着落：一句一个 `result`，或者都回显过（并进了
    /// 那一回合），或者等了 [`QUEUE_WAIT`] 还没动静（不回显的版本）。
    fn finished(&self) -> bool {
        if self.sent_messages == 0 || self.results == 0 || self.active_since_result {
            return false;
        }
        self.results >= self.sent_messages
            || self.unconsumed.is_empty()
            || self.last_result_at.is_some_and(|at| at.elapsed() >= QUEUE_WAIT)
    }

    /// 主线说完了，但还有后台子代理在跑、或者刚做完在等叫醒的那一轮：先别收。
    fn waiting_on_background(&mut self) -> bool {
        if !self.finished() || matches!(self.last_turn, Some((TurnEndReason::Interrupted, _))) {
            return false;
        }
        if self.background > 0 {
            let since = *self.waiting_since.get_or_insert_with(|| {
                self.sink.emit(EventBody::Notice {
                    level: super::super::event::NoticeLevel::Info,
                    text: format!("主线说完了，Claude Code 还有 {} 个后台子代理在跑：等它们做完再收尾（最多 30 分钟，STOP 能停）。", self.background),
                });
                tokio::time::Instant::now()
            });
            return since.elapsed() < BACKGROUND_WAIT;
        }
        match self.woken_at {
            Some((results, at)) => self.results <= results && at.elapsed() < WAKE_WAIT,
            None => false,
        }
    }

    pub(crate) async fn run(
        mut self,
        task: AgentTask,
        mut control: mpsc::UnboundedReceiver<Control>,
    ) -> Result<RunOutcome, GatewayError> {
        if let Err(e) = self.say(&task.prompt).await {
            emit_engine_died(&self.sink, "Claude Code", "turn-1", &self.pipe.stderr_tail());
            self.pipe.stop(Duration::from_secs(2)).await;
            return Err(e);
        }
        let mut interrupt_deadline: Option<tokio::time::Instant> = None;
        let mut died = false;
        while !self.finished() || self.waiting_on_background() {
            let deadline = async {
                match interrupt_deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            // 有过 `result` 以后每 5 秒醒一下，看等够了没有（等后台、等追加的那句）。
            let idle = self.results > 0;
            let tick = async move {
                if idle {
                    tokio::time::sleep(Duration::from_secs(5)).await
                } else {
                    std::future::pending().await
                }
            };
            tokio::select! {
                line = self.pipe.next() => match line {
                    Some(line) => self.handle(line),
                    None => {
                        died = true;
                        break;
                    }
                },
                Some(c) = control.recv() => {
                    if matches!(c, Control::Interrupt) && interrupt_deadline.is_none() {
                        interrupt_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    }
                    self.control(c).await;
                }
                _ = tick => {}
                _ = deadline => {
                    // 打断后 10 秒还没收尾：发 SIGINT，再结束进程。
                    self.pipe.signal("INT").await;
                    self.sink.emit(EventBody::TurnCompleted {
                        turn_id: self.last_turn.as_ref().map(|t| t.1.clone()).unwrap_or_else(|| "turn-1".into()),
                        reason: TurnEndReason::Interrupted,
                        error: None,
                    });
                    self.last_turn = Some((TurnEndReason::Interrupted, String::new()));
                    break;
                }
            }
        }
        if died {
            // 进程没了：转换器补收尾（没等到 result 的补一条出错的回合结束）。
            let tail = self.converter.finish();
            let bodies = emit_all(&self.sink, tail, None);
            let ended = bodies.iter().any(|b| matches!(b, EventBody::TurnCompleted { .. }));
            if !ended && !self.finished() {
                emit_engine_died(&self.sink, "Claude Code", "turn-1", &self.pipe.stderr_tail());
            }
            if !self.finished() {
                self.last_turn = Some((TurnEndReason::Error, String::new()));
            }
        }
        let reason = self.last_turn.map(|t| t.0).unwrap_or(TurnEndReason::Error);
        let session_ref = self.session_id.clone();
        self.pipe.stop(Duration::from_secs(5)).await;
        Ok(RunOutcome { reason, session_ref })
    }

    async fn say(&mut self, text: &str) -> Result<(), GatewayError> {
        self.pipe.send(&ClaudeCodeConverter::user_message(text)).await?;
        self.sent_messages += 1;
        self.unconsumed.push_back(text.to_string());
        Ok(())
    }

    /// 回合收尾要用的几个数：`result`、吃进去的话（回显）、`result` 之后主线又动了没有。
    fn track(&mut self, raw: &Value) {
        let main = raw["parent_tool_use_id"].is_null();
        match raw["type"].as_str().unwrap_or("") {
            "result" => {
                self.results += 1;
                self.last_result_at = Some(tokio::time::Instant::now());
                self.active_since_result = false;
            }
            "user" if raw["isReplay"] == true => {
                let content = &raw["message"]["content"];
                let text = content.as_str().map(str::to_string).or_else(|| {
                    content.as_array().map(|blocks| blocks.iter().filter_map(|b| b["text"].as_str()).collect::<String>())
                });
                if let Some(i) = text.and_then(|t| self.unconsumed.iter().position(|u| *u == t)) {
                    self.unconsumed.remove(i);
                }
                self.active_since_result = true;
            }
            "assistant" | "stream_event" | "control_request" if main => self.active_since_result = true,
            _ => {}
        }
    }

    fn handle(&mut self, line: Line) {
        let raw = match line {
            Line::Json(v) => v,
            Line::Text(_) => {
                self.sink.emit(EventBody::Raw {
                    source: "claude-code".into(),
                    kind: Some("unparsed".into()),
                });
                return;
            }
        };
        if raw["type"] == "control_request" && raw["request"]["subtype"] == "can_use_tool" {
            if let Some(id) = raw["request_id"].as_str() {
                let tool = raw["request"]["tool_name"].as_str().unwrap_or("").to_string();
                self.pending.insert(id.into(), (tool, raw["request"]["input"].clone()));
            }
        }
        // 真跑排查（只在测试构建里）：`PECK_LIVE_RAW=<文件>` 把原始的行一行一行记下来。
        #[cfg(test)]
        if let Ok(path) = std::env::var("PECK_LIVE_RAW") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{raw}");
            }
        }
        self.track(&raw);
        if self.compacting && raw["type"] == "system" && raw["subtype"] == "init" {
            let has = raw["slash_commands"].as_array().map(|c| c.iter().any(|x| x.as_str() == Some("compact")));
            if has == Some(false) {
                self.sink.emit(EventBody::Notice { level: NoticeLevel::Warn, text: "这个版本的 Claude Code 没有 /compact 命令，这句会当普通的话发过去。".into() });
            }
        }
        if raw["type"] == "system" && raw["subtype"] == "background_tasks_changed" {
            let agents = raw["tasks"].as_array().map(|t| t.iter().filter(|x| x["task_type"] == "local_agent").count()).unwrap_or(0);
            // 后台子代理做完了（个数变少到 0）：等 Claude Code 叫醒主线说完那一轮。
            if self.background > 0 && agents == 0 {
                self.woken_at = Some((self.results, tokio::time::Instant::now()));
            }
            self.background = agents;
        }
        let emitted = self.converter.feed(&raw);
        for body in emit_all(&self.sink, emitted, Some(&raw)) {
            match body {
                EventBody::SessionStarted { session_ref, .. } => {
                    if session_ref.is_some() {
                        self.session_id = session_ref;
                    }
                }
                EventBody::TurnCompleted { turn_id, reason, .. } => self.last_turn = Some((reason, turn_id)),
                _ => {}
            }
        }
    }

    async fn control(&mut self, c: Control) {
        let (c, by) = c.with_origin();
        match c {
            Control::Approve { request_id, decision } => {
                let Some((_, input)) = self.pending.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let reply = ClaudeCodeConverter::approval_reply(&request_id, decision, &input);
                if self.pipe.send(&reply).await.is_ok() {
                    self.sink.emit(EventBody::ApprovalResolved {
                        request_id,
                        decision,
                        by,
                    });
                }
            }
            // with_origin 已经换成了 Approve。
            Control::ApproveByRule { .. } => {}
            Control::Answer { request_id, answer, answers } => {
                if crate::gateway::mcp::try_answer(&request_id, &answer, &self.sink).await {
                    return;
                }
                let Some((_, input)) = self.pending.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let reply = ClaudeCodeConverter::answer_reply(&request_id, &input, &answer, &answers);
                if self.pipe.send(&reply).await.is_ok() {
                    let answer = crate::gateway::convert::ask_summary(input["questions"].as_array().map(Vec::as_slice).unwrap_or_default(), &answer, &answers);
                    self.sink.emit(EventBody::UserInputResolved { request_id, answer });
                }
            }
            Control::Append { text } => {
                if self.say(&text).await.is_err() {
                    self.sink.emit(EventBody::Notice {
                        level: NoticeLevel::Warn,
                        text: "这句没送进去：Claude Code 已经在收尾了。".into(),
                    });
                }
            }
            Control::Interrupt => {
                self.interrupts += 1;
                let id = format!("peck-interrupt-{}", self.interrupts);
                let _ = self.pipe.send(&ClaudeCodeConverter::interrupt_request(&id)).await;
            }
        }
    }

    fn unknown(&self, request_id: &str) {
        self.sink.emit(EventBody::Notice {
            level: NoticeLevel::Warn,
            text: format!("这件事（{request_id}）已经不用回答了。"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::{fixtures, Emitted};
    use crate::gateway::engines::testing::auto_answer;
    use crate::gateway::event::ApprovalDecision;
    use serde_json::json;
    use std::path::PathBuf;

    fn task() -> AgentTask {
        AgentTask {
            run_id: "test-run".into(),
            prompt: "In this folder: 1) read notes.txt".into(),
            cwd: PathBuf::from("{{WORKSPACE}}"),
            permission: PermissionLevel::Edit,
            model: Some("haiku".into()),
            effort: None,
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        }
    }

    #[tokio::test]
    async fn recorded_task_replays_through_the_engine() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("claude-code", tx, |_| ApprovalDecision::AllowOnce, "");
        let pipe = Pipe::script(&fixtures::read("claude-code-small-task", "raw.jsonl"));
        let t = task();
        let outcome = tokio::time::timeout(Duration::from_secs(5), Session::new(pipe, &t, sink).run(t, rx))
            .await
            .expect("回放卡住了")
            .unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert_eq!(outcome.session_ref.as_deref(), Some("2b7e9b65-57e7-41b2-890d-263e4a9a3315"));
        let got: Vec<Emitted> = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| Emitted { subagent_id: e.subagent_id.clone(), body: e.body.clone() })
            .collect();
        let want = crate::gateway::mock::parse_events_jsonl(&fixtures::read("claude-code-small-task", "events.jsonl")).unwrap();
        assert_eq!(got, want);
    }

    fn script(lines: &[Value]) -> Pipe {
        Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>())
    }

    fn result_line() -> Value {
        json!({"type": "result", "subtype": "success", "usage": {"input_tokens": 1, "output_tokens": 1}, "total_cost_usd": 0.001})
    }

    #[tokio::test]
    async fn approvals_and_questions_are_answered_over_stdio() {
        let pipe = script(&[
            json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-haiku-5-5", "tools": [], "mcp_servers": []}),
            json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "t1", "input": {"command": "rm -rf build"}}}),
            json!({"type": "control_request", "request_id": "r2", "request": {"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "tool_use_id": "t2",
                "input": {"questions": [{"question": "用哪个名字？", "header": "名字", "options": [{"label": "甲"}, {"label": "乙"}], "multiSelect": false}]}}}),
            result_line(),
        ]);
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("claude-code", tx, |_| ApprovalDecision::DenyOnce, "乙");
        let t = task();
        let session = Session::new(pipe, &t, sink);
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.run(t, rx)).await.unwrap().unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert_eq!(outcome.session_ref.as_deref(), Some("s1"));
        let events = events.lock().unwrap().clone();
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::UserInputRequested { options, .. } if options == &vec!["甲".to_string(), "乙".to_string()])));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ApprovalResolved { decision: ApprovalDecision::DenyOnce, .. })));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::UserInputResolved { answer, .. } if answer == "乙")));
    }

    #[test]
    fn replies_have_the_shape_claude_code_expects() {
        let deny = ClaudeCodeConverter::approval_reply("r1", ApprovalDecision::DenyOnce, &json!({"command": "x"}));
        assert_eq!(deny["response"]["request_id"], "r1");
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
        let input = json!({"questions": [{"question": "用哪个名字？", "options": [{"label": "甲"}]}]});
        let answer = ClaudeCodeConverter::answer_reply("r2", &input, "甲", &[]);
        let updated = &answer["response"]["response"]["updatedInput"];
        assert_eq!(answer["response"]["response"]["behavior"], "allow");
        assert_eq!(updated["answers"]["用哪个名字？"], "甲");
        assert_eq!(updated["questions"], input["questions"]);
        // 一次问两个（v1.1.9）：每个问题自己的答案；多选的几项连成一句。
        let two = json!({"questions": [{"question": "用哪个名字？", "options": [{"label": "甲"}]}, {"question": "要哪几样？", "multiSelect": true, "options": [{"label": "一"}, {"label": "二"}]}]});
        let each = vec![vec!["甲".to_string()], vec!["一".to_string(), "二".to_string()]];
        let both = ClaudeCodeConverter::answer_reply("r3", &two, "x", &each);
        assert_eq!(both["response"]["response"]["updatedInput"]["answers"], json!({"用哪个名字？": "甲", "要哪几样？": "一, 二"}));
        let args = ClaudeCodeBackend::args(&AgentTask { resume: Some("s1".into()), permission: PermissionLevel::Plan, ..task() });
        let joined = args.join(" ");
        assert!(joined.contains("--permission-prompt-tool stdio"));
        assert!(joined.contains("--replay-user-messages"));
        // 清单工具（`claude -p` 默认不给）只对这一次进程打开。
        let env = ClaudeCodeBackend::launch_env(&task(), &[]);
        assert!(env.contains(&("CLAUDE_CODE_ENABLE_TODO_TOOLS".to_string(), "1".to_string())));
        assert!(joined.contains("--permission-mode plan"));
        assert!(joined.contains("--model haiku") && joined.contains("--resume s1"));
        assert!(!joined.contains("--bare") && !joined.contains("--effort"));
        let with_effort = ClaudeCodeBackend::args(&AgentTask { effort: Some("high".into()), ..task() }).join(" ");
        // 委派时（v0.9 B1）带上 Peck 的本机 MCP，Peck 自己的工具不再问。
        assert!(!joined.contains("--mcp-config"));
        let mcp = crate::gateway::PeckMcp { url: "http://127.0.0.1:9/mcp".into(), token: "tok".into() };
        let with_mcp = ClaudeCodeBackend::args(&AgentTask { peck_mcp: Some(mcp), ..task() });
        let raw = with_mcp.iter().find_map(|a| a.strip_prefix("--mcp-config=")).unwrap();
        let cfg: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg["mcpServers"]["peck"]["type"], "http");
        assert_eq!(cfg["mcpServers"]["peck"]["headers"]["Authorization"], "Bearer ${PECK_MCP_TOKEN}", "令牌不进命令行");
        assert_eq!(cfg["mcpServers"]["peck"]["timeout"], 1_800_000);
        assert!(!with_mcp.join(" ").contains("tok\""));
        assert!(with_mcp.join(" ").contains("--allowedTools mcp__peck__ask_user,mcp__peck__create_skill"));
        assert!(with_effort.contains("--effort high"));
    }

    #[tokio::test]
    async fn background_subagents_are_waited_for_before_the_task_ends() {
        let changed = |n: usize| json!({"type": "system", "subtype": "background_tasks_changed", "session_id": "s1",
            "tasks": (0..n).map(|i| json!({"task_id": format!("a{i}"), "task_type": "local_agent", "description": "查一下"})).chain(std::iter::once(json!({"task_id": "sh", "task_type": "local_bash"}))).collect::<Vec<_>>()});
        let say = |text: &str| json!({"type": "assistant", "message": {"id": format!("m-{text}"), "role": "assistant", "content": [{"type": "text", "text": text}]}});
        let pipe = script(&[
            json!({"type": "system", "subtype": "init", "session_id": "s1", "tools": [], "mcp_servers": []}),
            changed(1),
            say("我让一个子代理在后台查。"),
            result_line(),
            changed(0),
            say("后台查完了：一共 3 块。"),
            result_line(),
        ]);
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("claude-code", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = task();
        let outcome = Session::new(pipe, &t, sink).run(t, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let events = events.lock().unwrap().clone();
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::Notice { text, .. } if text.contains("1 个后台子代理"))));
        // 叫醒的那一轮说的话也收到了（没在主线第一次说完时就关掉）。
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::MessageCompleted { text, .. } if text.contains("后台查完了"))));
    }

    /// 跑着时追加一句（v1.1.9 验收时真跑卡住过）：Claude Code 把它并进这一回合，只回一个 `result`。
    #[tokio::test]
    async fn an_appended_line_ends_with_the_turn_it_was_merged_into() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (sink, _events) = auto_answer("claude-code", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = task();
        let replay = |text: &str| Line::Json(json!({"type": "user", "isReplay": true, "parent_tool_use_id": null, "message": {"role": "user", "content": text}}));
        let result = || Line::Json(result_line());
        let started = || Line::Json(json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m2", "role": "assistant", "content": [{"type": "text", "text": "好"}]}}));

        // 两步之间吃进去（回显）、一个 result 收掉。
        let mut s = Session::new(script(&[]), &t, sink.clone());
        s.say("跑 sleep").await.unwrap();
        s.handle(replay("跑 sleep"));
        s.say("补一句").await.unwrap();
        assert!(!s.finished());
        s.handle(replay("补一句"));
        s.handle(result());
        assert!(s.finished());

        // 回合说完了才到：先一个 result，回显后开下一回合，第二个 result 才算完。
        let mut s = Session::new(script(&[]), &t, sink.clone());
        s.say("跑 sleep").await.unwrap();
        s.handle(replay("跑 sleep"));
        s.say("补一句").await.unwrap();
        s.handle(result());
        assert!(!s.finished(), "那一句还没吃进去");
        s.handle(replay("补一句"));
        s.handle(started());
        s.last_result_at = tokio::time::Instant::now().checked_sub(QUEUE_WAIT * 2);
        assert!(!s.finished(), "下一回合在跑，不能收");
        s.handle(result());
        assert!(s.finished());

        // 不回显的版本：result 之后一直没动静，等够了也收。
        let mut s = Session::new(script(&[]), &t, sink);
        s.say("跑 sleep").await.unwrap();
        s.say("补一句").await.unwrap();
        s.handle(result());
        assert!(!s.finished());
        s.last_result_at = tokio::time::Instant::now().checked_sub(QUEUE_WAIT);
        assert!(s.finished());
    }

    #[tokio::test]
    async fn engine_exit_without_result_ends_with_an_error_turn() {
        let pipe = script(&[json!({"type": "system", "subtype": "init", "session_id": "s1", "tools": [], "mcp_servers": []})]);
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("claude-code", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = task();
        let outcome = Session::new(pipe, &t, sink).run(t, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Error);
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert!(matches!(last.body, EventBody::TurnCompleted { reason: TurnEndReason::Error, .. }));
    }

    /// 真跑一个小任务（本地，平时不跑）：`PECK_LIVE_CLAUDE=<claude 路径> PECK_LIVE_CWD=<测试项目>
    /// PECK_LIVE_PROMPT=…`，`PECK_LIVE_MODE=default` 让命令弹审批（走 stdio）。审批一律批准一次。
    #[tokio::test]
    #[ignore]
    async fn live_small_task() {
        let bin = PathBuf::from(std::env::var("PECK_LIVE_CLAUDE").expect("PECK_LIVE_CLAUDE"));
        let cwd = PathBuf::from(std::env::var("PECK_LIVE_CWD").expect("PECK_LIVE_CWD"));
        let prompt = std::env::var("PECK_LIVE_PROMPT").expect("PECK_LIVE_PROMPT");
        let mut env = vec![("DISABLE_AUTOUPDATER".to_string(), "1".to_string())];
        if let Ok(dir) = std::env::var("PECK_LIVE_CONFIG_DIR") {
            env.push(("CLAUDE_CONFIG_DIR".into(), dir));
        }
        let path = crate::agent::build_augmented_path(&[bin.parent().unwrap().to_string_lossy().into_owned()]);
        let t = AgentTask {
            run_id: "live".into(),
            prompt,
            cwd: cwd.clone(),
            permission: PermissionLevel::Edit,
            model: Some("haiku".into()),
            effort: None,
            resume: std::env::var("PECK_LIVE_RESUME").ok(),
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: std::env::var("PECK_LIVE_COMPACT").is_ok(),
            trusted: std::env::var("PECK_LIVE_TRUST").is_ok(),
        };
        let mut args = ClaudeCodeBackend::args(&t);
        if let Ok(mode) = std::env::var("PECK_LIVE_MODE") {
            let i = args.iter().position(|a| a == "--permission-mode").unwrap();
            args[i + 1] = mode;
        }
        args.extend(["--max-turns".into(), "8".into()]);
        let pipe = Pipe::spawn(&Launch { bin, args, cwd, env, path }).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("claude-code", tx, |_| ApprovalDecision::AllowOnce, "好");
        let outcome = Session::new(pipe, &t, sink).run(t, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }
}
