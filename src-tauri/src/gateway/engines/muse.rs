//! Muse：会话协议 MSP（`muse serve`，stdio JSON-RPC）。比 `muse exec --json` 多了审批、提问、
//! 追加、打断和续接，所以 v0.5 直接用它（计划里写的 exec 不用了）。
//!
//! 一个回合：`initialize` → `initialized` → `session/start`（续接用 `session/resume`）→ `turn/start`
//! → 收通知直到这个回合的 `turn/completed`。Muse 发来的请求（`approval/request`、`userInput/request`）
//! 先回一个空的「收到」，站主的决定另外用 `approval/decide`、`userInput/answer` 发。
//! 每个命令都带一个 UUIDv7 的 `commandId`（Muse 要求）。
//!
//! 权限三档 → `approvalMode`：PLAN = `denyUnmatched`（规则没放行的一律拒），EDIT = `promptUnmatched`
//! （规则没放行的先给 Muse 的审查员看，拿不准的问 Peck），AUTO = `onRequest`。

use super::{emit_all, emit_engine_died, is_rpc_response, EngineSetup, Launch, Line, Pipe};
use crate::gateway::caps::AgentCaps;
use crate::gateway::convert::muse::MuseConverter;
use crate::gateway::convert::LineConverter;
use crate::gateway::event::{EventBody, NoticeLevel, PermissionLevel, ResolvedBy, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, BoxFuture, Control, ErrorKind, GatewayError, RunOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub struct MuseBackend {
    setup: EngineSetup,
}

impl MuseBackend {
    pub fn new(setup: EngineSetup) -> Self {
        Self { setup }
    }
}

impl AgentBackend for MuseBackend {
    fn id(&self) -> &str {
        "muse"
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::muse()
    }

    fn run(
        &self,
        task: AgentTask,
        sink: EventSink,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let launch = Launch {
            bin: self.setup.bin.clone(),
            args: vec!["serve".into()],
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

/// UUIDv7：前 48 位是毫秒时间戳（Muse 的 `commandId` 要这种）。
pub fn uuid_v7() -> String {
    let mut b = *uuid::Uuid::new_v4().as_bytes();
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    for (i, byte) in b.iter_mut().take(6).enumerate() {
        *byte = (ms >> (8 * (5 - i))) as u8;
    }
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(b).to_string()
}

struct PendingApproval {
    approval_id: String,
    choices: Value,
    requirement: Value,
}

pub(crate) struct Session {
    pipe: Pipe,
    sink: EventSink,
    converter: MuseConverter,
    next_id: u64,
    session_id: Option<String>,
    turn_id: Option<String>,
    approvals: HashMap<String, PendingApproval>,
    questions: HashMap<String, (String, Vec<String>)>,
    finished: Option<TurnEndReason>,
}

impl Session {
    pub(crate) fn new(pipe: Pipe, task: &AgentTask, sink: EventSink) -> Self {
        Self {
            pipe,
            sink,
            converter: MuseConverter::new(Some(task.cwd.to_string_lossy().into_owned())),
            next_id: 1,
            session_id: None,
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
        if let Err(e) = self.start(&task).await {
            let tail = self.pipe.stderr_tail();
            let detail = match tail.lines().last() {
                Some(l) if !l.trim().is_empty() => format!("{}（{l}）", e.message),
                _ => e.message.clone(),
            };
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

    async fn drive(mut self, control: &mut mpsc::UnboundedReceiver<Control>) -> Result<RunOutcome, GatewayError> {
        let mut interrupt_deadline: Option<tokio::time::Instant> = None;
        while self.finished.is_none() {
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
                        emit_engine_died(&self.sink, "Muse", &turn, &self.pipe.stderr_tail());
                        self.finished = Some(TurnEndReason::Error);
                    }
                },
                Some(c) = control.recv() => {
                    if matches!(c, Control::Interrupt) && interrupt_deadline.is_none() {
                        interrupt_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    }
                    self.control(c).await;
                }
                _ = deadline => {
                    self.sink.emit(EventBody::TurnCompleted {
                        turn_id: self.turn_id.clone().unwrap_or_default(),
                        reason: TurnEndReason::Interrupted,
                        error: None,
                    });
                    self.finished = Some(TurnEndReason::Interrupted);
                }
            }
        }
        let reason = self.finished.take().unwrap_or(TurnEndReason::Error);
        let session_ref = self.session_id.clone();
        self.pipe.stop(Duration::from_secs(3)).await;
        Ok(RunOutcome { reason, session_ref })
    }

    async fn start(&mut self, task: &AgentTask) -> Result<(), GatewayError> {
        self.request(
            "initialize",
            json!({"clientInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"userInputDialogs": true}}),
        )
        .await?;
        self.pipe.send(&json!({"jsonrpc": "2.0", "method": "initialized"})).await?;
        let mode = match task.permission {
            PermissionLevel::Plan => "denyUnmatched",
            PermissionLevel::Edit => "promptUnmatched",
            PermissionLevel::Auto => "onRequest",
        };
        let cwd = task.cwd.to_string_lossy().into_owned();
        let session = match &task.resume {
            Some(id) => {
                let mut p = json!({"commandId": uuid_v7(), "sessionId": id, "history": "auto", "excludeItems": true});
                if let Some(model) = &task.model {
                    p["modelId"] = json!(model);
                }
                self.request("session/resume", p).await?
            }
            None => {
                let mut p = json!({"commandId": uuid_v7(), "workspaceRoots": [cwd], "approvalMode": mode});
                if let Some(model) = &task.model {
                    p["modelId"] = json!(model);
                }
                self.request("session/start", p).await?
            }
        };
        self.session_id = session["session"]["sessionId"].as_str().map(str::to_string).or_else(|| task.resume.clone());
        let Some(session_id) = self.session_id.clone() else {
            return Err(GatewayError::new(ErrorKind::Engine, "Muse 没给会话号。"));
        };
        let mut params = json!({
            "commandId": uuid_v7(),
            "sessionId": session_id,
            "input": [{"type": "text", "text": task.prompt}],
        });
        if let Some(effort) = &task.effort {
            params["reasoningEffort"] = json!(effort);
        }
        let turn = self.request("turn/start", params).await?;
        self.turn_id = turn["turnId"].as_str().map(str::to_string);
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, GatewayError> {
        let id = self.next_id;
        self.next_id += 1;
        self.pipe
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let Some(line) = self.pipe.next().await else {
                return Err(GatewayError::new(ErrorKind::Engine, "Muse 起来以后马上退出了。"));
            };
            if let Line::Json(v) = &line {
                if is_rpc_response(v) && v["id"] == json!(id) {
                    if let Some(message) = v["error"]["message"].as_str() {
                        return Err(GatewayError::new(
                            ErrorKind::Engine,
                            format!("Muse 拒绝了 {method}：{}", crate::gateway::convert::short(message, 200)),
                        ));
                    }
                    return Ok(v["result"].clone());
                }
            }
            self.handle(line).await;
        }
    }

    /// 不等回应地发一个命令（回应来了当普通的行处理：出错的会变成一条错误事件）。
    async fn command(&mut self, method: &str, params: Value) -> Result<(), GatewayError> {
        let id = self.next_id;
        self.next_id += 1;
        self.pipe
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await
    }

    async fn handle(&mut self, line: Line) {
        let raw = match line {
            Line::Json(v) => v,
            Line::Text(_) => {
                self.sink.emit(EventBody::Raw {
                    source: "muse".into(),
                    kind: Some("unparsed".into()),
                });
                return;
            }
        };
        // Muse 发来的请求：先回「收到」（它只当展示回执），决定另外发。
        if let (Some(_), Some(id)) = (raw["method"].as_str(), raw.get("id")) {
            let _ = self.pipe.send(&json!({"jsonrpc": "2.0", "id": id, "result": {}})).await;
        }
        let p = &raw["params"];
        match raw["method"].as_str() {
            Some("approval/request") | Some("approval/requested") => {
                if let Some(approval) = p["approvalId"].as_str() {
                    let key = MuseConverter::approval_id(approval);
                    if p["availableChoices"].is_array() || !self.approvals.contains_key(&key) {
                        self.approvals.insert(
                            key,
                            PendingApproval {
                                approval_id: approval.into(),
                                choices: p["availableChoices"].clone(),
                                requirement: p["currentRequirementId"].clone(),
                            },
                        );
                    }
                }
            }
            Some("approval/resolved") => {
                if let Some(approval) = p["approvalId"].as_str() {
                    self.approvals.remove(&MuseConverter::approval_id(approval));
                }
            }
            Some("userInput/request") => {
                if let Some(input) = p["userInputId"].as_str() {
                    let ids = p["questions"]
                        .as_array()
                        .map(|q| q.iter().filter_map(|x| x["id"].as_str().map(str::to_string)).collect())
                        .unwrap_or_default();
                    self.questions.insert(MuseConverter::input_id(input), (input.into(), ids));
                }
            }
            _ => {}
        }
        let emitted = self.converter.feed(&raw);
        for body in emit_all(&self.sink, emitted, Some(&raw)) {
            if let EventBody::TurnCompleted { turn_id, reason, .. } = body {
                if self.turn_id.as_deref().is_none_or(|t| t == turn_id) {
                    self.finished = Some(reason);
                }
            }
        }
    }

    async fn control(&mut self, c: Control) {
        let Some(session) = self.session_id.clone() else {
            return;
        };
        match c {
            Control::Approve { request_id, decision } => {
                let Some(pending) = self.approvals.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let Some(choice) = MuseConverter::choice_for(decision, &pending.choices) else {
                    return self.unknown(&request_id);
                };
                let sent = self
                    .command(
                        "approval/decide",
                        json!({"commandId": uuid_v7(), "sessionId": session, "approvalId": pending.approval_id,
                               "choiceId": choice, "requirementId": pending.requirement}),
                    )
                    .await;
                if sent.is_ok() {
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
                let Some((input, questions)) = self.questions.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                let answers: Vec<Value> = questions
                    .iter()
                    .map(|q| json!({"questionId": q, "freeText": answer}))
                    .collect();
                let sent = self
                    .command(
                        "userInput/answer",
                        json!({"commandId": uuid_v7(), "sessionId": session, "userInputId": input, "answers": answers}),
                    )
                    .await;
                if sent.is_ok() {
                    self.sink.emit(EventBody::UserInputResolved { request_id, answer });
                }
            }
            Control::Append { text } => {
                let Some(turn) = self.turn_id.clone() else { return };
                let _ = self
                    .command(
                        "turn/steer",
                        json!({"commandId": uuid_v7(), "sessionId": session, "expectedTurnId": turn,
                               "input": [{"type": "text", "text": text}]}),
                    )
                    .await;
            }
            Control::Interrupt => {
                let mut p = json!({"commandId": uuid_v7(), "sessionId": session});
                if let Some(turn) = &self.turn_id {
                    p["turnId"] = json!(turn);
                }
                let _ = self.command("turn/interrupt", p).await;
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

    fn task() -> AgentTask {
        AgentTask {
            run_id: "test-run".into(),
            prompt: "Read notes.txt".into(),
            cwd: PathBuf::from("{{WORKSPACE}}"),
            permission: PermissionLevel::Edit,
            model: None,
            effort: Some("low".into()),
            resume: None,
            peck_mcp: None,
        }
    }

    /// 录音时多发了 `model/list`、`usage/read`（id 2、3）；引擎不发，回放前去掉这两条回应、后面的 id 往前挪。
    fn engine_view(raw: &str) -> String {
        raw.lines()
            .filter_map(|l| {
                let mut v: Value = serde_json::from_str(l).ok()?;
                if is_rpc_response(&v) {
                    match v["id"].as_u64() {
                        Some(2) | Some(3) => return None,
                        Some(n) if n > 3 => v["id"] = json!(n - 2),
                        _ => {}
                    }
                }
                Some(v.to_string() + "\n")
            })
            .collect()
    }

    #[tokio::test]
    async fn recorded_task_replays_through_the_engine() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("muse", tx, |_| ApprovalDecision::AllowOnce, "");
        let pipe = Pipe::script(&engine_view(&fixtures::read("muse-small-task", "raw.jsonl")));
        let t = task();
        let outcome = tokio::time::timeout(Duration::from_secs(5), Session::new(pipe, &t, sink).run(t, rx))
            .await
            .expect("回放卡住了")
            .unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(outcome.session_ref.is_some());
        let got: Vec<Emitted> = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| Emitted { subagent_id: e.subagent_id.clone(), body: e.body.clone() })
            .collect();
        let want = crate::gateway::mock::parse_events_jsonl(&fixtures::read("muse-small-task", "events.jsonl")).unwrap();
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn escalated_approval_is_received_then_decided_by_command() {
        let lines = [
            json!({"id": 1, "result": {"serverInfo": {"name": "muse"}}}),
            json!({"method": "session/started", "params": {"session": {"sessionId": "s1", "modelId": "muse-spark-1.3"}}}),
            json!({"id": 2, "result": {"session": {"sessionId": "s1"}}}),
            json!({"id": 3, "result": {"turnId": "t1", "status": "accepted"}}),
            json!({"method": "turn/started", "params": {"turnId": "t1"}}),
            json!({"method": "approval/request", "id": 0, "params": {"sessionId": "s1", "approvalId": "a1", "toolCallId": "c1", "toolName": "bash",
                "subject": {"kind": "shell", "command": "rm notes.txt"}, "currentRequirementId": {"approvalId": "a1", "sourceIndex": 0},
                "availableChoices": [{"choiceId": "allow_once", "decision": "approved", "scope": "once"}, {"choiceId": "abort", "decision": "abort", "scope": "once"}]}}),
            // approval/decide 是第 4 个请求：等 Peck 发了它，回放才往下走（真 Muse 也是等决定）。
            json!({"id": 4, "result": {"approvalId": "a1", "status": "accepted", "terminal": true}}),
            json!({"method": "turn/completed", "params": {"turnId": "t1", "terminal": "completed"}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("muse", tx, |_| ApprovalDecision::DenyOnce, "");
        let t = task();
        let mut session = Session::new(pipe, &t, sink);
        session.start(&t).await.unwrap();
        assert_eq!(session.pipe.sent()[2]["params"]["approvalMode"], "promptUnmatched");
        let mut rx = rx;
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.drive(&mut rx)).await.unwrap().unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let events = events.lock().unwrap().clone();
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ApprovalRequested { request_id, .. } if request_id == "muse-a1")));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ApprovalResolved { decision: ApprovalDecision::DenyOnce, .. })));
    }

    #[test]
    fn command_ids_are_uuid_v7() {
        let id = uuid_v7();
        let parsed = uuid::Uuid::parse_str(&id).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
        assert_ne!(id, uuid_v7());
    }

    /// 真跑一个小任务（本地，平时不跑）：`PECK_LIVE_MUSE=<muse 路径> PECK_LIVE_CWD=… PECK_LIVE_PROMPT=…`，
    /// 可选 `PECK_LIVE_MODEL`。审批一律批准一次。
    #[tokio::test]
    #[ignore]
    async fn live_small_task() {
        let bin = PathBuf::from(std::env::var("PECK_LIVE_MUSE").expect("PECK_LIVE_MUSE"));
        let setup = EngineSetup {
            path: crate::agent::build_augmented_path(&[bin.parent().unwrap().to_string_lossy().into_owned()]),
            bin,
            env: vec![],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("muse", tx, |_| ApprovalDecision::AllowOnce, "好");
        let t = AgentTask {
            run_id: "live".into(),
            prompt: std::env::var("PECK_LIVE_PROMPT").expect("PECK_LIVE_PROMPT"),
            cwd: PathBuf::from(std::env::var("PECK_LIVE_CWD").expect("PECK_LIVE_CWD")),
            permission: PermissionLevel::Edit,
            model: std::env::var("PECK_LIVE_MODEL").ok(),
            effort: Some("low".into()),
            resume: None,
            peck_mcp: None,
        };
        let outcome = MuseBackend::new(setup).run(t, sink, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }
}
