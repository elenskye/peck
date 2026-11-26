//! Grok：ACP（Agent Client Protocol，stdio JSON-RPC，`grok agent stdio`）。
//!
//! 一个回合：`initialize` → `session/new`（续接用 `session/resume`，不回放历史）→ 按任务设模型、
//! 强度、模式（`session/set_config_option`）→ `session/prompt`，它的回应就是这一轮的结束。
//! 审批是 ACP 发来的 `session/request_permission`，回答就是这个请求的结果。打断发 `session/cancel`
//! 通知，还没回答的审批一律回「取消」。ACP 一轮里不能再追加一句（不支持 `Control::Append`）。
//! 强度是 `reasoning_effort`；没有模式，权限靠 Peck 回答审批。

use super::{emit_all, emit_engine_died, is_rpc_response, EngineSetup, Launch, Line, Pipe};
use crate::gateway::caps::AgentCaps;
use crate::gateway::convert::acp::AcpConverter;
use crate::gateway::convert::LineConverter;
use crate::gateway::event::{ApprovalDecision, EventBody, NoticeLevel, ResolvedBy, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, BoxFuture, Control, ErrorKind, GatewayError, RunOutcome};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

/// 走 ACP 的引擎（现在只有 Grok）。
#[derive(Debug, Clone)]
pub struct Flavor {
    pub engine: &'static str,
    pub label: &'static str,
    pub args: &'static [&'static str],
    pub effort_option: &'static str,
}

pub const GROK: Flavor = Flavor { engine: "grok", label: "Grok", args: &["agent", "stdio"], effort_option: "reasoning_effort" };

pub struct AcpBackend {
    setup: EngineSetup,
    flavor: Flavor,
}

impl AcpBackend {
    pub fn new(setup: EngineSetup, flavor: Flavor) -> Self {
        Self { setup, flavor }
    }
}

impl AgentBackend for AcpBackend {
    fn setup(&self) -> Option<&EngineSetup> {
        Some(&self.setup)
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::for_engine(self.flavor.engine)
    }

    fn run(
        &self,
        task: AgentTask,
        sink: EventSink,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let launch = Launch {
            bin: self.setup.bin.clone(),
            args: self.flavor.args.iter().map(|s| s.to_string()).collect(),
            cwd: task.cwd.clone(),
            env: super::project_env(&task).into_iter().chain(self.setup.env.iter().cloned()).collect(),
            path: self.setup.path.clone(),
        };
        let flavor = self.flavor.clone();
        Box::pin(async move {
            let pipe = Pipe::spawn(&launch)?;
            Session::new(pipe, flavor, &task, sink).run(task, control).await
        })
    }
}

pub(crate) struct Session {
    pipe: Pipe,
    flavor: Flavor,
    sink: EventSink,
    converter: AcpConverter,
    next_id: u64,
    session_id: Option<String>,
    prompt_id: Option<u64>,
    /// 等站主回答的审批：统一的 request id → (JSON-RPC id, 引擎给的选项)。
    approvals: HashMap<String, (Value, Value)>,
    finished: Option<TurnEndReason>,
}

impl Session {
    pub(crate) fn new(pipe: Pipe, flavor: Flavor, task: &AgentTask, sink: EventSink) -> Self {
        let mut converter = AcpConverter::new(flavor.engine, Some(task.cwd.to_string_lossy().into_owned()));
        converter.compacting = task.compact;
        Self {
            pipe,
            flavor,
            sink,
            converter,
            next_id: 1,
            session_id: None,
            prompt_id: None,
            approvals: HashMap::new(),
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
                turn_id: "turn-1".into(),
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
                        emit_engine_died(&self.sink, self.flavor.label, "turn-1", &self.pipe.stderr_tail());
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
                        turn_id: "turn-1".into(),
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
            json!({"protocolVersion": 1,
                   "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                   "clientInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
        let cwd = task.cwd.to_string_lossy().into_owned();
        // MCP 入口：Peck 的本机 MCP 起来了就加进去（ask_user）。
        let mcp: Vec<Value> = task
            .peck_mcp
            .iter()
            .map(|m| json!({"type": "http", "name": "peck", "url": m.url, "headers": [{"name": "Authorization", "value": format!("Bearer {}", m.token)}]}))
            .collect();
        match &task.resume {
            Some(id) => {
                self.request("session/resume", json!({"sessionId": id, "cwd": cwd, "mcpServers": mcp})).await?;
                self.session_id = Some(id.clone());
                // resume 的回应不带会话号：自己补一条会话开始。
                self.sink.emit(EventBody::SessionStarted {
                    engine: self.flavor.engine.into(),
                    model: task.model.clone(),
                    caps: AgentCaps::for_engine(self.flavor.engine),
                    tools: vec![],
                    mcp: vec![],
                    session_ref: Some(id.clone()),
                    version: None,
                });
            }
            None => {
                let r = self.request("session/new", json!({"cwd": cwd, "mcpServers": mcp})).await?;
                self.session_id = r["sessionId"].as_str().map(str::to_string);
            }
        }
        let Some(session) = self.session_id.clone() else {
            return Err(GatewayError::new(ErrorKind::Engine, format!("{} 没给会话号。", self.flavor.label)));
        };
        let mut options: Vec<(&str, String)> = Vec::new();
        if let Some(model) = &task.model {
            options.push(("model", model.clone()));
        }
        if let Some(effort) = &task.effort {
            options.push((self.flavor.effort_option, effort.clone()));
        }
        for (id, value) in options {
            if let Err(e) = self
                .request("session/set_config_option", json!({"sessionId": session, "configId": id, "value": value}))
                .await
            {
                // 设不上不算致命：照默认的跑，告诉站主一声。
                self.sink.emit(EventBody::Notice {
                    level: NoticeLevel::Warn,
                    text: format!("{} 没接受 {id} = {value}：{}", self.flavor.label, e.message),
                });
            }
        }
        for e in self.converter.start_turn() {
            self.sink.emit(e);
        }
        // 转发 `/compact`（v1.1.7）：它广告的 `compact` 命令不带参数，重点不传。
        let text = if task.compact {
            if task.prompt.trim().trim_start_matches("/compact").trim() != "" {
                self.sink.emit(EventBody::Notice { level: NoticeLevel::Info, text: format!("{} 不接收重点，按它自己的方式压。", self.flavor.label) });
            }
            "/compact".to_string()
        } else {
            task.prompt.clone()
        };
        let id = self.next_id;
        self.next_id += 1;
        self.prompt_id = Some(id);
        self.pipe
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": "session/prompt",
                          "params": {"sessionId": session, "prompt": [{"type": "text", "text": text}]}}))
            .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, GatewayError> {
        let id = self.next_id;
        self.next_id += 1;
        self.pipe
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let Some(line) = self.pipe.next().await else {
                return Err(GatewayError::new(ErrorKind::Engine, format!("{} 起来以后马上退出了。", self.flavor.label)));
            };
            if let Line::Json(v) = &line {
                if is_rpc_response(v) && v["id"] == json!(id) {
                    if let Some(message) = v["error"]["message"].as_str() {
                        let auth = v["error"]["code"] == json!(-32000) || message.to_lowercase().contains("auth");
                        return Err(GatewayError::new(
                            if auth { ErrorKind::Auth } else { ErrorKind::Engine },
                            format!("{} 拒绝了 {method}：{}", self.flavor.label, crate::gateway::convert::short(message, 200)),
                        ));
                    }
                    // session/new 的回应要进转换器（会话开始）；其余的不用。
                    if method == "session/new" {
                        self.handle(line.clone()).await;
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
                    source: self.flavor.engine.into(),
                    kind: Some("unparsed".into()),
                });
                return;
            }
        };
        if let (Some(method), Some(id)) = (raw["method"].as_str(), raw.get("id")) {
            if method == "session/request_permission" {
                let key = AcpConverter::request_id(self.flavor.engine, id);
                self.approvals.insert(key, (id.clone(), raw["params"]["options"].clone()));
            } else {
                // 不认识的请求（fs、terminal 我们没声明）：马上回错误，免得它干等。
                let _ = self
                    .pipe
                    .send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Peck 不支持 {method}")}}))
                    .await;
            }
        }
        let emitted = self.converter.feed(&raw);
        for body in emit_all(&self.sink, emitted, Some(&raw)) {
            if let EventBody::TurnCompleted { reason, .. } = body {
                self.finished = Some(reason);
            }
        }
        // 回合的回应出错（不是 stopReason）：补一条出错的回合结束。
        if is_rpc_response(&raw) && raw["id"].as_u64() == self.prompt_id && raw.get("error").is_some() && self.finished.is_none() {
            let message = raw["error"]["message"].as_str().unwrap_or("出错了").to_string();
            self.sink.emit(EventBody::TurnCompleted {
                turn_id: "turn-1".into(),
                reason: TurnEndReason::Error,
                error: Some(message),
            });
            self.finished = Some(TurnEndReason::Error);
        }
    }

    async fn control(&mut self, c: Control) {
        let (c, by) = c.with_origin();
        match c {
            Control::Approve { request_id, decision } => {
                let Some((id, options)) = self.approvals.remove(&request_id) else {
                    return super::stale(&self.sink, &request_id);
                };
                let reply = AcpConverter::permission_reply(&id, decision, &options);
                if self.pipe.send(&reply).await.is_ok() {
                    self.sink.emit(EventBody::ApprovalResolved { request_id, decision, by });
                }
            }
            // with_origin 已经换成了 Approve。
            Control::ApproveByRule { .. } => {}
            Control::Answer { request_id, answer, .. } => {
                // ACP 自己不问站主；提问都来自 Peck 的本机 MCP（ask_user）。
                if !crate::gateway::mcp::try_answer(&request_id, &answer, &self.sink).await {
                    super::stale(&self.sink, &request_id);
                }
            }
            Control::Append { .. } => {
                self.notice(format!("{} 这一轮里插不了话。", self.flavor.label));
            }
            Control::Interrupt => {
                // 还没回答的审批先回「取消」，再发 session/cancel。
                let pending: Vec<(String, (Value, Value))> = self.approvals.drain().collect();
                for (request_id, (id, _)) in pending {
                    let _ = self
                        .pipe
                        .send(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {"outcome": "cancelled"}}}))
                        .await;
                    self.sink.emit(EventBody::ApprovalResolved {
                        request_id,
                        decision: ApprovalDecision::DenyOnce,
                        by: ResolvedBy::User,
                    });
                }
                if let Some(session) = self.session_id.clone() {
                    let _ = self
                        .pipe
                        .send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}))
                        .await;
                }
            }
        }
    }

    fn notice(&self, text: String) {
        self.sink.emit(EventBody::Notice { level: NoticeLevel::Warn, text });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::{fixtures, Emitted};
    use crate::gateway::engines::testing::auto_answer;
    use crate::gateway::event::PermissionLevel;
    use std::path::PathBuf;

    fn task(model: &str, effort: &str) -> AgentTask {
        AgentTask {
            run_id: "test-run".into(),
            prompt: "Read notes.txt".into(),
            cwd: PathBuf::from("{{WORKSPACE}}"),
            permission: PermissionLevel::Edit,
            model: Some(model.into()),
            effort: Some(effort.into()),
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        }
    }

    async fn replay(flavor: Flavor, name: &str, t: AgentTask) -> (RunOutcome, Vec<Emitted>, Vec<Value>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer(flavor.engine, tx, |_| ApprovalDecision::AllowOnce, "");
        let raw = fixtures::read(name, "raw.jsonl");
        let pipe = Pipe::script(&raw);
        let mut session = Session::new(pipe, flavor, &t, sink);
        session.start(&t).await.unwrap();
        let sent = session.pipe.sent().to_vec();
        let mut rx = rx;
        let outcome = tokio::time::timeout(Duration::from_secs(5), session.drive(&mut rx))
            .await
            .expect("回放卡住了")
            .unwrap();
        let got = events
            .lock()
            .unwrap()
            .iter()
            .map(|e| Emitted { subagent_id: e.subagent_id.clone(), body: e.body.clone() })
            .collect();
        (outcome, got, sent)
    }

    fn without_resolved(events: Vec<Emitted>) -> Vec<Emitted> {
        events.into_iter().filter(|e| !matches!(e.body, EventBody::ApprovalResolved { .. })).collect()
    }

    #[tokio::test]
    async fn grok_recorded_task_replays_through_the_engine() {
        let (outcome, got, sent) = replay(GROK, "grok-small-task", task("grok-4.5", "low")).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(outcome.session_ref.is_some());
        assert_eq!(sent[2]["params"], json!({"sessionId": sent[2]["params"]["sessionId"], "configId": "model", "value": "grok-4.5"}));
        assert_eq!(sent[3]["params"]["configId"], "reasoning_effort");
        let want = crate::gateway::mock::parse_events_jsonl(&fixtures::read("grok-small-task", "events.jsonl")).unwrap();
        assert_eq!(without_resolved(got.clone()), want);
        assert_eq!(got.len() - want.len(), 1, "一次审批、一条 approval.resolved");
    }

    /// 真跑一个小任务（本地，平时不跑）：`PECK_LIVE_BIN=… PECK_LIVE_CWD=…
    /// PECK_LIVE_PROMPT=… PECK_LIVE_MODEL=… PECK_LIVE_EFFORT=…`。审批一律批准一次。
    #[tokio::test]
    #[ignore]
    async fn live_small_task() {
        let flavor = GROK;
        let bin = PathBuf::from(std::env::var("PECK_LIVE_BIN").expect("PECK_LIVE_BIN"));
        let setup = EngineSetup {
            path: crate::agent::build_augmented_path(&[bin.parent().unwrap().to_string_lossy().into_owned()]),
            bin,
            env: vec![],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer(flavor.engine, tx, |_| ApprovalDecision::AllowOnce, "蓝");
        // `PECK_LIVE_MCP=1`：起 Peck 的本机 MCP，让引擎能调 ask_user（答案一律「蓝」）。
        let peck_mcp = match std::env::var("PECK_LIVE_MCP").as_deref() {
            Ok("1") => Some(crate::gateway::mcp::hub().await.unwrap().register(sink.clone())),
            _ => None,
        };
        let t = AgentTask {
            run_id: "live".into(),
            prompt: std::env::var("PECK_LIVE_PROMPT").expect("PECK_LIVE_PROMPT"),
            cwd: PathBuf::from(std::env::var("PECK_LIVE_CWD").expect("PECK_LIVE_CWD")),
            permission: PermissionLevel::Edit,
            model: std::env::var("PECK_LIVE_MODEL").ok(),
            effort: std::env::var("PECK_LIVE_EFFORT").ok(),
            resume: std::env::var("PECK_LIVE_RESUME").ok(),
            peck_mcp,
            env: vec![],
            folders: vec![],
            compact: std::env::var("PECK_LIVE_COMPACT").is_ok(),
            trusted: std::env::var("PECK_LIVE_TRUST").is_ok(),
        };
        let outcome = AcpBackend::new(setup, flavor).run(t, sink, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }

    /// 转发 `/compact`（v1.1.7）：发的是不带重点的 `/compact`，写了重点就说一声。
    #[tokio::test]
    async fn compact_sends_the_bare_command() {
        let lines = [
            json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 1}}),
            json!({"jsonrpc": "2.0", "id": 2, "result": {}}),
            json!({"jsonrpc": "2.0", "method": "_x.ai/session_notification", "params": {"sessionId": "s1", "update": {"sessionUpdate": "auto_compact_completed", "tokens_before": 50000, "tokens_after": 9000}}}),
            json!({"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "end_turn"}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("grok", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = AgentTask { resume: Some("s1".into()), compact: true, prompt: "/compact 留住接口那段".into(), model: None, effort: None, ..task("grok-4.5", "low") };
        let mut session = Session::new(pipe, GROK, &t, sink);
        session.start(&t).await.unwrap();
        let prompt = session.pipe.sent().iter().find(|s| s["method"] == "session/prompt").cloned().unwrap();
        assert_eq!(prompt["params"]["prompt"][0]["text"], "/compact");
        let outcome = session.drive(&mut { rx }).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let bodies: Vec<EventBody> = events.lock().unwrap().iter().map(|e| e.body.clone()).collect();
        assert!(bodies.iter().any(|b| matches!(b, EventBody::Notice { text, .. } if text.contains("不接收重点"))));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ContextCompacted { before: 50000, after: 9000, text, .. } if text.contains("Grok"))));
    }

}
