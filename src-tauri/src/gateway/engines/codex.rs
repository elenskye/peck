//! Codex：`codex app-server`（stdio JSON-RPC，协议按本机版本 `codex app-server generate-json-schema`）。
//!
//! 一个回合：`initialize` → `initialized` → `thread/start`（续接用 `thread/resume`）→ `turn/start`
//! → 收通知直到这个回合的 `turn/completed`。审批（`requestApproval`）、提问（`requestUserInput`）
//! 是 Codex 发来的请求，等控制通道回答；追加一句用 `turn/steer`，打断用 `turn/interrupt`。
//!
//! 权限三档：PLAN = 只读沙箱，EDIT = 可写工作区，AUTO = 可写工作区 + 联网；审批都交给 Peck
//! （PLAN、EDIT 用 `untrusted`，AUTO 用 `on-request`）。

use super::{emit_all, emit_engine_died, is_rpc_response, EngineSetup, Launch, Line, Pipe};
use crate::gateway::caps::AgentCaps;
use crate::gateway::convert::codex::CodexConverter;
use crate::gateway::convert::LineConverter;
use crate::gateway::event::{EventBody, PermissionLevel, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, BoxFuture, Control, ErrorKind, GatewayError, RunOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct CodexBackend {
    setup: EngineSetup,
}

impl CodexBackend {
    pub fn new(setup: EngineSetup) -> Self {
        Self { setup }
    }
}

impl AgentBackend for CodexBackend {
    fn setup(&self) -> Option<&EngineSetup> {
        Some(&self.setup)
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::codex()
    }

    fn run(
        &self,
        task: AgentTask,
        sink: EventSink,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let (mcp_args, mcp_env) = peck_mcp_args(&task);
        let mut args: Vec<String> = vec!["app-server".into()];
        args.extend(mcp_args);
        // 站主批过这个项目（v1.1.8）：这一次进程把它当 trusted（读项目里的 `.codex/`）。
        if task.trusted {
            args.extend(crate::gateway::trust::launch_args("codex", &task.cwd));
        }
        let mut env = super::project_env(&task);
        env.extend(self.setup.env.iter().cloned());
        env.extend(mcp_env);
        let launch = Launch { bin: self.setup.bin.clone(), args, cwd: task.cwd.clone(), env, path: self.setup.path.clone() };
        Box::pin(async move {
            let pipe = Pipe::spawn(&launch)?;
            Session::new(pipe, &task, sink).run(task, control).await
        })
    }
}

/// 一次回合的会话。测试里换成回放的 [`Pipe`]。
pub(crate) struct Session {
    pipe: Pipe,
    sink: EventSink,
    converter: CodexConverter,
    next_id: u64,
    thread_id: Option<String>,
    turn_id: Option<String>,
    /// 等站主回答的审批：统一的 request id → Codex 的 JSON-RPC id。
    approvals: HashMap<String, Value>,
    /// 里面哪些是 MCP 工具的审批（回答的格式不一样）。
    elicitations: std::collections::HashSet<String>,
    /// 等站主回答的提问：统一的 request id → (JSON-RPC id, 问题 id)。
    questions: HashMap<String, (Value, Vec<Value>)>,
    finished: Option<(TurnEndReason, Option<String>)>,
}

impl Session {
    pub(crate) fn new(pipe: Pipe, task: &AgentTask, sink: EventSink) -> Self {
        let mut converter = CodexConverter::new(Some(task.cwd.to_string_lossy().into_owned()));
        converter.compacting = task.compact;
        Self {
            pipe,
            sink,
            converter,
            next_id: 1,
            thread_id: None,
            turn_id: None,
            approvals: HashMap::new(),
            elicitations: Default::default(),
            questions: HashMap::new(),
            finished: None,
        }
    }

    pub(crate) async fn run(
        mut self,
        task: AgentTask,
        mut control: mpsc::UnboundedReceiver<Control>,
    ) -> Result<RunOutcome, GatewayError> {
        let started = self.start(&task).await;
        if let Err(e) = started {
            let tail = self.pipe.stderr_tail();
            let detail = if tail.is_empty() { e.message.clone() } else { format!("{}（{}）", e.message, tail.lines().last().unwrap_or("")) };
            self.sink.emit(EventBody::Error { message: detail.clone(), fatal: true });
            self.sink.emit(EventBody::TurnCompleted {
                turn_id: self.turn_id.clone().unwrap_or_default(),
                reason: TurnEndReason::Error,
                error: Some(detail),
            });
            self.pipe.stop(Duration::from_secs(2)).await;
            return Err(e);
        }
        self.drive(&mut control).await
    }

    /// 回合开始以后：收通知、回控制，直到这个回合结束。
    async fn drive(
        mut self,
        control: &mut mpsc::UnboundedReceiver<Control>,
    ) -> Result<RunOutcome, GatewayError> {
        let mut interrupt_deadline: Option<tokio::time::Instant> = None;
        loop {
            if self.finished.is_some() {
                break;
            }
            let deadline = async {
                match interrupt_deadline {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                line = self.pipe.next() => match line {
                    Some(line) => self.handle(line).await,
                    None => {
                        let turn = self.turn_id.clone().unwrap_or_default();
                        emit_engine_died(&self.sink, "Codex", &turn, &self.pipe.stderr_tail());
                        self.finished = Some((TurnEndReason::Error, None));
                    }
                },
                Some(c) = control.recv() => {
                    if matches!(c, Control::Interrupt) && interrupt_deadline.is_none() {
                        interrupt_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    }
                    self.control(c).await;
                }
                _ = deadline => {
                    // 打断后 10 秒还没收尾：结束进程。
                    self.sink.emit(EventBody::TurnCompleted {
                        turn_id: self.turn_id.clone().unwrap_or_default(),
                        reason: TurnEndReason::Interrupted,
                        error: None,
                    });
                    self.finished = Some((TurnEndReason::Interrupted, None));
                }
            }
        }
        let (reason, _) = self.finished.take().unwrap_or((TurnEndReason::Error, None));
        let session_ref = self.thread_id.clone();
        self.pipe.stop(Duration::from_secs(3)).await;
        Ok(RunOutcome { reason, session_ref })
    }

    async fn start(&mut self, task: &AgentTask) -> Result<(), GatewayError> {
        self.request(
            "initialize",
            json!({"clientInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")}, "capabilities": null}),
        )
        .await?;
        self.pipe.send(&json!({"jsonrpc": "2.0", "method": "initialized"})).await?;
        let cwd = task.cwd.to_string_lossy().into_owned();
        let (sandbox, approval) = match task.permission {
            PermissionLevel::Plan => ("read-only", "untrusted"),
            PermissionLevel::Edit => ("workspace-write", "untrusted"),
            PermissionLevel::Auto => ("workspace-write", "on-request"),
        };
        let mut thread_params = json!({"cwd": cwd, "approvalPolicy": approval, "sandbox": sandbox});
        if let Some(model) = &task.model {
            thread_params["model"] = json!(model);
        }

        // 转发 `/compact`（v1.1.7）：续上会话、发 `thread/compact/start`（不收重点）；进度照常走 `turn/*`、`item/*`。
        if task.compact {
            let Some(id) = &task.resume else {
                return Err(GatewayError::new(ErrorKind::Engine, "这个任务还没有 Codex 的会话，没什么可压的。"));
            };
            if task.prompt.trim().trim_start_matches("/compact").trim() != "" {
                self.sink.emit(EventBody::Notice { level: crate::gateway::event::NoticeLevel::Info, text: "Codex 不接收重点，按它自己的方式压。".into() });
            }
            thread_params["threadId"] = json!(id);
            thread_params["excludeTurns"] = json!(true);
            let thread = self.request("thread/resume", thread_params).await?;
            self.thread_id = thread["thread"]["id"].as_str().map(str::to_string).or(Some(id.clone()));
            let thread_id = self.thread_id.clone().unwrap_or_default();
            self.request("thread/compact/start", json!({"threadId": thread_id})).await?;
            return Ok(());
        }
        let thread = match &task.resume {
            Some(id) => {
                thread_params["threadId"] = json!(id);
                // 只要会话号、不要整段历史（整段读回 Codex 说已弃用，每次续接都报一条，v0.10 验收发现）。
                thread_params["excludeTurns"] = json!(true);
                self.request("thread/resume", thread_params).await?
            }
            None => self.request("thread/start", thread_params).await?,
        };
        self.thread_id = thread["thread"]["id"].as_str().map(str::to_string);
        let Some(thread_id) = self.thread_id.clone() else {
            return Err(GatewayError::new(ErrorKind::Engine, "Codex 没给会话号。"));
        };
        let mut sandbox_policy = match task.permission {
            PermissionLevel::Plan => json!({"type": "readOnly"}),
            PermissionLevel::Edit => json!({"type": "workspaceWrite", "networkAccess": false}),
            PermissionLevel::Auto => json!({"type": "workspaceWrite", "networkAccess": true}),
        };
        // 项目里能改的其他文件夹（v0.10）：这一回合的沙箱多放开这几个（只读的它本来就读得到）。
        let roots = super::writable_folders(task);
        if !roots.is_empty() {
            sandbox_policy["writableRoots"] = json!(roots);
        }
        let mut params = json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": task.prompt, "text_elements": []}],
            "approvalPolicy": approval,
            "sandboxPolicy": sandbox_policy,
        });
        if let Some(model) = &task.model {
            params["model"] = json!(model);
        }
        if let Some(effort) = &task.effort {
            params["effort"] = json!(effort);
        }
        let turn = self.request("turn/start", params).await?;
        self.turn_id = turn["turn"]["id"].as_str().map(str::to_string);
        Ok(())
    }

    /// 发一个请求，等它的回应；等的时候来的通知照常处理。
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, GatewayError> {
        let id = self.next_id;
        self.next_id += 1;
        self.pipe
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let Some(line) = self.pipe.next().await else {
                return Err(GatewayError::new(ErrorKind::Engine, "Codex 起来以后马上退出了。"));
            };
            if let Line::Json(v) = &line {
                if is_rpc_response(v) && v["id"] == json!(id) {
                    if let Some(message) = v["error"]["message"].as_str() {
                        return Err(GatewayError::new(
                            ErrorKind::Engine,
                            format!("Codex 拒绝了 {method}：{}", crate::gateway::convert::short(message, 200)),
                        ));
                    }
                    return Ok(v["result"].clone());
                }
            }
            self.handle(line).await;
        }
    }

    async fn handle(&mut self, line: Line) {
        let raw = match line {
            Line::Json(v) => v,
            Line::Text(_) => {
                self.sink.emit(EventBody::Raw {
                    source: "codex".into(),
                    kind: Some("unparsed".into()),
                });
                return;
            }
        };
        let emitted = self.converter.feed(&raw);
        let bodies = emit_all(&self.sink, emitted, Some(&raw));
        // Codex 发来的请求：记下来等站主；Peck 不认识的马上回错误，免得它干等。
        if let (Some(method), Some(id)) = (raw["method"].as_str(), raw.get("id")) {
            let mut known = false;
            for body in &bodies {
                match body {
                    EventBody::ApprovalRequested { request_id, .. } => {
                        self.approvals.insert(request_id.clone(), id.clone());
                        if CodexConverter::is_mcp_approval(&raw) {
                            self.elicitations.insert(request_id.clone());
                        }
                        known = true;
                    }
                    EventBody::UserInputRequested { request_id, .. } => {
                        // 问题原样留着：回答时照顺序一个个对上（v1.1.9）。
                        let asked = raw["params"]["questions"].as_array().cloned().unwrap_or_default();
                        self.questions.insert(request_id.clone(), (id.clone(), asked));
                        known = true;
                    }
                    _ => {}
                }
            }
            if !known {
                let _ = self
                    .pipe
                    .send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Peck 还不支持 {method}")}}))
                    .await;
            }
        }
        for body in bodies {
            if let EventBody::TurnCompleted { turn_id, reason, error } = body {
                let ours = self.turn_id.as_deref().is_none_or(|t| t == turn_id);
                if ours {
                    self.finished = Some((reason, error));
                }
            }
        }
    }

    async fn control(&mut self, c: Control) {
        let (c, by) = c.with_origin();
        match c {
            Control::Approve { request_id, decision } => {
                let Some(id) = self.approvals.remove(&request_id) else {
                    return super::stale(&self.sink, &request_id);
                };
                let result = if self.elicitations.remove(&request_id) {
                    CodexConverter::elicitation_reply(decision)
                } else {
                    CodexConverter::decision_reply(decision)
                };
                let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
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
            Control::Answer { request_id, answer, answers: each } => {
                if crate::gateway::mcp::try_answer(&request_id, &answer, &self.sink).await {
                    return;
                }
                let Some((id, asked)) = self.questions.remove(&request_id) else {
                    return super::stale(&self.sink, &request_id);
                };
                let answers: serde_json::Map<String, Value> = asked
                    .iter()
                    .enumerate()
                    .filter_map(|(i, q)| Some((q["id"].as_str()?.to_string(), json!({"answers": Control::picks(&answer, &each, i)}))))
                    .collect();
                let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"answers": answers}});
                if self.pipe.send(&reply).await.is_ok() {
                    let answer = crate::gateway::convert::ask_summary(&asked, &answer, &each);
                    self.sink.emit(EventBody::UserInputResolved { request_id, answer });
                }
            }
            Control::Append { text } => {
                let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone()) else {
                    return;
                };
                let id = self.next_id;
                self.next_id += 1;
                let _ = self
                    .pipe
                    .send(&json!({"jsonrpc": "2.0", "id": id, "method": "turn/steer", "params": {
                        "threadId": thread, "expectedTurnId": turn,
                        "input": [{"type": "text", "text": text, "text_elements": []}]}}))
                    .await;
            }
            Control::Interrupt => {
                let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone()) else {
                    return;
                };
                let id = self.next_id;
                self.next_id += 1;
                let _ = self
                    .pipe
                    .send(&json!({"jsonrpc": "2.0", "id": id, "method": "turn/interrupt", "params": {"threadId": thread, "turnId": turn}}))
                    .await;
            }
        }
    }

}

/// Peck 的本机 MCP（v0.9 B1，委派时才给）：`codex app-server -c …` 只对这一次进程生效，不改 Codex 的配置文件；
/// 令牌走环境变量 `PECK_MCP_TOKEN`（`bearer_token_env_var`）。子任务要跑几分钟，工具超时放到 30 分钟。
/// （`codex mcp get peck -c …` 核实过这几个键，2026-11-16。）
fn peck_mcp_args(task: &AgentTask) -> (Vec<String>, Vec<(String, String)>) {
    let Some(m) = task.peck_mcp.as_ref() else { return (vec![], vec![]) };
    let set = |k: &str, v: String| ["-c".to_string(), format!("mcp_servers.peck.{k}={v}")];
    let mut args = Vec::new();
    args.extend(set("url", format!("{:?}", m.url)));
    args.extend(set("bearer_token_env_var", "\"PECK_MCP_TOKEN\"".into()));
    args.extend(set("startup_timeout_sec", "20".into()));
    args.extend(set("tool_timeout_sec", "1800".into()));
    (args, vec![("PECK_MCP_TOKEN".into(), m.token.clone())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::{fixtures, Emitted};
    use crate::gateway::engines::testing::auto_answer;
    use crate::gateway::event::ApprovalDecision;
    use std::path::PathBuf;

    fn task(permission: PermissionLevel) -> AgentTask {
        AgentTask {
            run_id: "test-run".into(),
            prompt: "In this folder: 1) read notes.txt".into(),
            cwd: PathBuf::from("{{WORKSPACE}}"),
            permission,
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        }
    }

    #[tokio::test]
    async fn recorded_task_replays_through_the_engine_with_three_approvals() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "");
        let pipe = Pipe::script(&fixtures::read("codex-small-task", "raw.jsonl"));
        let t = task(PermissionLevel::Edit);
        let session = Session::new(pipe, &t, sink);
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.run(t, rx))
            .await
            .expect("回放卡住了")
            .unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert_eq!(outcome.session_ref.as_deref(), Some("01a1cde8-386e-7ae0-85f0-88094adfe84e"));

        let events = events.lock().unwrap().clone();
        let resolved = events
            .iter()
            .filter(|e| matches!(e.body, EventBody::ApprovalResolved { by: crate::gateway::event::ResolvedBy::User, .. }))
            .count();
        assert_eq!(resolved, 3);
        // 去掉引擎这一层加的 approval.resolved，剩下的和转换器的夹具结果一模一样。
        let got: Vec<Emitted> = events
            .into_iter()
            .filter(|e| !matches!(e.body, EventBody::ApprovalResolved { .. }))
            .map(|e| Emitted { subagent_id: e.subagent_id, body: e.body })
            .collect();
        let want = crate::gateway::mock::parse_events_jsonl(&fixtures::read("codex-small-task", "events.jsonl")).unwrap();
        assert_eq!(crate::gateway::engines::testing::without_estimates(got), want);
    }

    #[tokio::test]
    async fn sends_permission_model_and_decisions_the_codex_way() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, _events) = auto_answer(
            "codex",
            tx,
            |title| if title.contains("stamp") { ApprovalDecision::DenyOnce } else { ApprovalDecision::AllowAlways },
            "",
        );
        let pipe = Pipe::script(&fixtures::read("codex-small-task", "raw.jsonl"));
        let t = task(PermissionLevel::Auto);
        let mut session = Session::new(pipe, &t, sink);
        session.start(&t).await.unwrap();
        let sent = session.pipe.sent().to_vec();
        assert_eq!(sent[0]["method"], "initialize");
        assert_eq!(sent[1]["method"], "initialized");
        assert_eq!(sent[2]["params"]["sandbox"], "workspace-write");
        assert_eq!(sent[2]["params"]["approvalPolicy"], "on-request");
        // 委派时（v0.9 B1）：本机 MCP 用 -c 只给这一次进程，令牌走环境变量。
        assert!(peck_mcp_args(&t).0.is_empty());
        let with = AgentTask { peck_mcp: Some(crate::gateway::PeckMcp { url: "http://127.0.0.1:9/mcp".into(), token: "tok".into() }), ..t.clone() };
        let (args, env) = peck_mcp_args(&with);
        assert_eq!(args[0..2], ["-c".to_string(), "mcp_servers.peck.url=\"http://127.0.0.1:9/mcp\"".to_string()]);
        assert!(args.contains(&"mcp_servers.peck.bearer_token_env_var=\"PECK_MCP_TOKEN\"".to_string()));
        assert!(!args.iter().any(|a| a.contains("tok\"")) && env == vec![("PECK_MCP_TOKEN".to_string(), "tok".to_string())]);

        let turn = &sent[3]["params"];
        assert_eq!(turn["sandboxPolicy"], json!({"type": "workspaceWrite", "networkAccess": true}));
        assert_eq!((turn["model"].as_str(), turn["effort"].as_str()), (Some("gpt-6-luna"), Some("low")));

        let mut rx = rx;
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.drive(&mut rx)).await.unwrap().unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
    }

    #[tokio::test]
    async fn resuming_asks_for_no_history() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (sink, _events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "");
        let pipe = Pipe::script(&fixtures::read("codex-small-task", "raw.jsonl"));
        let t = AgentTask { resume: Some("thread-1".into()), ..task(PermissionLevel::Edit) };
        let mut session = Session::new(pipe, &t, sink);
        let _ = session.start(&t).await;
        let sent = session.pipe.sent().to_vec();
        assert_eq!(sent[2]["method"], "thread/resume");
        assert_eq!((sent[2]["params"]["threadId"].as_str(), sent[2]["params"]["excludeTurns"].as_bool()), (Some("thread-1"), Some(true)));
    }

    #[tokio::test]
    async fn engine_exit_mid_turn_ends_with_an_error_turn() {
        // 只留到 turn/start 的回应：后面 Codex「退出」了。
        let raw = fixtures::read("codex-small-task", "raw.jsonl");
        let cut: String = raw.lines().take(10).map(|l| format!("{l}\n")).collect();
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = task(PermissionLevel::Edit);
        let outcome = Session::new(Pipe::script(&cut), &t, sink).run(t, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Error);
        let last = events.lock().unwrap().last().cloned().unwrap();
        assert!(matches!(last.body, EventBody::TurnCompleted { reason: TurnEndReason::Error, .. }));
    }

    /// 真跑一个小任务（本地，平时不跑）：`PECK_LIVE_CODEX=<codex 路径> PECK_LIVE_CWD=<测试项目>`。
    /// 花一个 Codex 小任务的授权；审批一律批准一次。
    #[tokio::test]
    #[ignore]
    async fn live_small_task() {
        let bin = PathBuf::from(std::env::var("PECK_LIVE_CODEX").expect("PECK_LIVE_CODEX"));
        let cwd = PathBuf::from(std::env::var("PECK_LIVE_CWD").expect("PECK_LIVE_CWD"));
        let prompt = std::env::var("PECK_LIVE_PROMPT").expect("PECK_LIVE_PROMPT");
        let setup = EngineSetup {
            path: crate::agent::build_augmented_path(&[bin.parent().unwrap().to_string_lossy().into_owned()]),
            bin,
            env: vec![],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "好");
        let t = AgentTask {
            run_id: "live".into(),
            prompt,
            cwd,
            permission: PermissionLevel::Edit,
            // 默认最便宜的那档；`PECK_LIVE_MODEL=default` 用 Codex 自己的默认（不带 --model）。
            model: match std::env::var("PECK_LIVE_MODEL").as_deref() {
                Ok("default") => None,
                Ok(m) => Some(m.to_string()),
                Err(_) => Some("gpt-6-luna".into()),
            },
            effort: match std::env::var("PECK_LIVE_EFFORT").as_deref() {
                Ok("default") => None,
                Ok(e) => Some(e.to_string()),
                Err(_) => Some("low".into()),
            },
            resume: std::env::var("PECK_LIVE_RESUME").ok(),
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: std::env::var("PECK_LIVE_COMPACT").is_ok(),
            trusted: std::env::var("PECK_LIVE_TRUST").is_ok(),
        };
        let outcome = CodexBackend::new(setup).run(t, sink, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }
    /// 真跑收尾 Hook（v1.1.0，本地，平时不跑）：`PECK_LIVE_CODEX=<codex 路径> PECK_LIVE_CWD=<一个 git 仓库>`。
    /// Codex 改一个文件，再续上它的会话、只读地问 Git Plan 的那一句，Peck 用答回来的那一行提交。
    /// 花一个 Codex 小任务的授权（两轮）；审批一律批准一次。
    #[tokio::test]
    #[ignore]
    async fn live_stop_hook_commits_the_round() {
        use crate::harness::{git, hooks};
        let bin = PathBuf::from(std::env::var("PECK_LIVE_CODEX").expect("PECK_LIVE_CODEX"));
        let cwd = PathBuf::from(std::env::var("PECK_LIVE_CWD").expect("PECK_LIVE_CWD"));
        let setup = EngineSetup { path: crate::agent::build_augmented_path(&[bin.parent().unwrap().to_string_lossy().into_owned()]), bin, env: vec![] };
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "好");
        let t = AgentTask {
            run_id: "live-hook".into(),
            prompt: "在 notes.txt 末尾加一行「hook smoke」。只改这一个文件，不要运行任何 git 命令。".into(),
            cwd: cwd.clone(),
            permission: PermissionLevel::Edit,
            model: Some(std::env::var("PECK_LIVE_MODEL").unwrap_or_else(|_| "gpt-6-luna".into())),
            effort: Some("low".into()),
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        };
        let backend = CodexBackend::new(setup);
        let outcome = backend.run(t.clone(), sink, rx).await.expect("第一轮");
        println!("round: {:?}, {} events", outcome.reason, events.lock().unwrap().len());
        let files = git::changed(&cwd).unwrap();
        println!("changed: {files:?}");
        // 问的就是库里 Git Plan 那一句。
        let hooks_json: serde_json::Value = serde_json::from_str(include_str!("../../../defaults/library/plugins/git-plan/hooks/hooks.json")).unwrap();
        let ask = hooks_json["hooks"]["Stop"].as_array().unwrap().iter().find_map(|h| h["hooks"][0]["ask"].as_str()).unwrap().to_string();
        let repos = hooks::pending(&[(cwd.clone(), files)], PermissionLevel::Edit, true, false);
        let again = AgentTask { resume: outcome.session_ref.clone(), ..t };
        let text = hooks::ask_cli(&backend, "codex", again, &hooks::ask_for(&ask, &repos), |body| println!("usage: {}", serde_json::to_string(&body).unwrap().chars().take(160).collect::<String>())).await;
        println!("answer: {text:?}");
        let hook = hooks::StopHook { plugin: "Git Plan".into(), ask };
        let answer = text.map(|text| hooks::Answer { roots: repos.iter().map(|r| r.root.clone()).collect(), text });
        for repo in &repos {
            println!("{}", serde_json::to_string(&hooks::commit(&hook, repo, answer.as_ref())).unwrap());
        }
        println!("uncommitted now: {:?}", git::changed(&cwd).unwrap());
    }

    /// 转发 `/compact`（v1.1.7）：续上会话、发 `thread/compact/start`（不发 `turn/start`、不带重点）；压缩那一条、用量照常转。
    #[tokio::test]
    async fn compact_is_forwarded_with_its_own_request() {
        let lines = [
            json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
            json!({"jsonrpc": "2.0", "id": 2, "result": {"thread": {"id": "thread-1"}}}),
            json!({"jsonrpc": "2.0", "id": 3, "result": {}}),
            json!({"jsonrpc": "2.0", "method": "turn/started", "params": {"threadId": "thread-1", "turn": {"id": "turn-c"}}}),
            json!({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {"type": "contextCompaction", "id": "cc1"}}}),
            json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {"type": "contextCompaction", "id": "cc1"}}}),
            json!({"jsonrpc": "2.0", "method": "thread/tokenUsage/updated", "params": {"tokenUsage": {"last": {"totalTokens": 9000, "inputTokens": 8000, "cachedInputTokens": 0, "outputTokens": 1000, "reasoningOutputTokens": 0}, "total": {"totalTokens": 90000, "inputTokens": 80000, "cachedInputTokens": 0, "outputTokens": 10000, "reasoningOutputTokens": 0}, "modelContextWindow": 400000}}}),
            json!({"jsonrpc": "2.0", "method": "turn/completed", "params": {"turn": {"id": "turn-c", "status": "completed"}}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("codex", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = AgentTask { resume: Some("thread-1".into()), compact: true, prompt: "/compact 留住数据库那段".into(), ..task(PermissionLevel::Edit) };
        let outcome = Session::new(pipe, &t, sink).run(t, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert_eq!(outcome.session_ref.as_deref(), Some("thread-1"));
        let bodies: Vec<EventBody> = events.lock().unwrap().iter().map(|e| e.body.clone()).collect();
        assert!(bodies.iter().any(|b| matches!(b, EventBody::Notice { text, .. } if text.contains("不接收重点"))));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ContextCompacted { step: crate::gateway::event::CompactStep::Engine, text, .. } if text.contains("Codex") && text.contains("你让它压的"))));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ContextUpdated { used: 9000, window: Some(400000), managed_by: Some(m), .. } if m == "Codex")), "用量用 last.totalTokens");
    }

}
