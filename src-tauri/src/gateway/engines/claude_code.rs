//! Claude Code：`claude -p` 的双向 stream-json。用站主登录的订阅，不加 `--bare`（照平时读 CLAUDE.md、
//! hooks、MCP）。
//!
//! 一个回合：起进程 → 写一行 `user` → 读到 `result` 为止。审批用 `--permission-prompt-tool stdio`：
//! Claude Code 发 `control_request`（`can_use_tool`），Peck 回 `control_response`；`AskUserQuestion`
//! 也走这条路，回答放进 `updatedInput.answers`。追加一句就再写一行 `user`（它排在这一回合后面，
//! 等它的 `result` 也到了才算完）；打断发 `control_request`（`interrupt`）。
//!
//! 权限三档：PLAN = `plan`，EDIT = `acceptEdits`，AUTO 也是 `acceptEdits`（放宽靠 Peck 自动批，
//! 高危照样弹卡片）。

use super::{emit_all, emit_engine_died, EngineSetup, Launch, Line, Pipe};
use crate::gateway::caps::AgentCaps;
use crate::gateway::convert::claude_code::ClaudeCodeConverter;
use crate::gateway::convert::LineConverter;
use crate::gateway::event::{EventBody, NoticeLevel, PermissionLevel, ResolvedBy, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, BoxFuture, Control, GatewayError, RunOutcome};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct ClaudeCodeBackend {
    setup: EngineSetup,
}

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
        args
    }
}

impl AgentBackend for ClaudeCodeBackend {
    fn id(&self) -> &str {
        "claude-code"
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
        let mut env = vec![("DISABLE_AUTOUPDATER".to_string(), "1".to_string())];
        env.extend(self.setup.env.iter().cloned());
        let launch = Launch {
            bin: self.setup.bin.clone(),
            args: Self::args(&task),
            cwd: task.cwd.clone(),
            env,
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
    /// 写进去的 `user` 行数和收到的 `result` 数：一样多才算这一回合完了。
    sent_messages: usize,
    results: usize,
    last_turn: Option<(TurnEndReason, String)>,
    interrupts: u32,
}

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
            last_turn: None,
            interrupts: 0,
        }
    }

    fn finished(&self) -> bool {
        self.sent_messages > 0 && self.results >= self.sent_messages
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
        while !self.finished() {
            let deadline = async {
                match interrupt_deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
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
        Ok(())
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
        if raw["type"] == "result" {
            self.results += 1;
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
                        by: ResolvedBy::User,
                    });
                }
            }
            Control::Answer { request_id, answer } => {
                if crate::gateway::mcp::try_answer(&request_id, &answer, &self.sink).await {
                    return;
                }
                let Some((_, input)) = self.pending.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let reply = ClaudeCodeConverter::answer_reply(&request_id, &input, &answer);
                if self.pipe.send(&reply).await.is_ok() {
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
        let answer = ClaudeCodeConverter::answer_reply("r2", &input, "甲");
        let updated = &answer["response"]["response"]["updatedInput"];
        assert_eq!(answer["response"]["response"]["behavior"], "allow");
        assert_eq!(updated["answers"]["用哪个名字？"], "甲");
        assert_eq!(updated["questions"], input["questions"]);
        let args = ClaudeCodeBackend::args(&AgentTask { resume: Some("s1".into()), permission: PermissionLevel::Plan, ..task() });
        let joined = args.join(" ");
        assert!(joined.contains("--permission-prompt-tool stdio"));
        assert!(joined.contains("--permission-mode plan"));
        assert!(joined.contains("--model haiku") && joined.contains("--resume s1"));
        assert!(!joined.contains("--bare") && !joined.contains("--effort"));
        let with_effort = ClaudeCodeBackend::args(&AgentTask { effort: Some("high".into()), ..task() }).join(" ");
        assert!(with_effort.contains("--effort high"));
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
            resume: None,
            peck_mcp: None,
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
