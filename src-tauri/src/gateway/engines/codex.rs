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
use crate::gateway::event::{EventBody, NoticeLevel, PermissionLevel, ResolvedBy, TurnEndReason};
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
    fn id(&self) -> &str {
        "codex"
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
        let launch = Launch {
            bin: self.setup.bin.clone(),
            args: vec!["app-server".into()],
            cwd: task.cwd.clone(),
            env: self.setup.env.clone(),
            path: self.setup.path.clone(),
        };
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
    /// 等站主回答的提问：统一的 request id → (JSON-RPC id, 问题 id)。
    questions: HashMap<String, (Value, Vec<String>)>,
    finished: Option<(TurnEndReason, Option<String>)>,
}

impl Session {
    pub(crate) fn new(pipe: Pipe, task: &AgentTask, sink: EventSink) -> Self {
        Self {
            pipe,
            sink,
            converter: CodexConverter::new(Some(task.cwd.to_string_lossy().into_owned())),
            next_id: 1,
            thread_id: None,
            turn_id: None,
            approvals: HashMap::new(),
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
        let thread = match &task.resume {
            Some(id) => {
                thread_params["threadId"] = json!(id);
                self.request("thread/resume", thread_params).await?
            }
            None => self.request("thread/start", thread_params).await?,
        };
        self.thread_id = thread["thread"]["id"].as_str().map(str::to_string);
        let Some(thread_id) = self.thread_id.clone() else {
            return Err(GatewayError::new(ErrorKind::Engine, "Codex 没给会话号。"));
        };
        let sandbox_policy = match task.permission {
            PermissionLevel::Plan => json!({"type": "readOnly"}),
            PermissionLevel::Edit => json!({"type": "workspaceWrite", "networkAccess": false}),
            PermissionLevel::Auto => json!({"type": "workspaceWrite", "networkAccess": true}),
        };
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
                        known = true;
                    }
                    EventBody::UserInputRequested { request_id, .. } => {
                        let ids = raw["params"]["questions"]
                            .as_array()
                            .map(|q| q.iter().filter_map(|x| x["id"].as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        self.questions.insert(request_id.clone(), (id.clone(), ids));
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
        match c {
            Control::Approve { request_id, decision } => {
                let Some(id) = self.approvals.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let reply = json!({"jsonrpc": "2.0", "id": id, "result": CodexConverter::decision_reply(decision)});
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
                let Some((id, question_ids)) = self.questions.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let answers: serde_json::Map<String, Value> = question_ids
                    .into_iter()
                    .map(|q| (q, json!({"answers": [answer.clone()]})))
                    .collect();
                let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"answers": answers}});
                if self.pipe.send(&reply).await.is_ok() {
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
            .filter(|e| matches!(e.body, EventBody::ApprovalResolved { by: ResolvedBy::User, .. }))
            .count();
        assert_eq!(resolved, 3);
        // 去掉引擎这一层加的 approval.resolved，剩下的和转换器的夹具结果一模一样。
        let got: Vec<Emitted> = events
            .into_iter()
            .filter(|e| !matches!(e.body, EventBody::ApprovalResolved { .. }))
            .map(|e| Emitted { subagent_id: e.subagent_id, body: e.body })
            .collect();
        let want = crate::gateway::mock::parse_events_jsonl(&fixtures::read("codex-small-task", "events.jsonl")).unwrap();
        assert_eq!(got, want);
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
        let turn = &sent[3]["params"];
        assert_eq!(turn["sandboxPolicy"], json!({"type": "workspaceWrite", "networkAccess": true}));
        assert_eq!((turn["model"].as_str(), turn["effort"].as_str()), (Some("gpt-6-luna"), Some("low")));

        let mut rx = rx;
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.drive(&mut rx)).await.unwrap().unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
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
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
            resume: None,
            peck_mcp: None,
        };
        let outcome = CodexBackend::new(setup).run(t, sink, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }
}
