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
use crate::gateway::event::{EventBody, NoticeLevel, PermissionLevel, TurnEndReason};
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
    fn setup(&self) -> Option<&EngineSetup> {
        Some(&self.setup)
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
            // 站主批过这个项目（v1.1.8）：`--trust-workspace`，只这一次进程。
            args: std::iter::once("serve".to_string()).chain(task.trusted.then(|| crate::gateway::trust::launch_args("muse", &task.cwd)).into_iter().flatten()).collect(),
            cwd: task.cwd.clone(),
            env: super::project_env(&task).into_iter().chain(self.setup.env.iter().cloned()).collect(),
            path: self.setup.path.clone(),
        };
        Box::pin(async move {
            let pipe = Pipe::spawn(&launch)?;
            Session::new(pipe, &task, sink).run(task, control).await
        })
    }
}

/// Peck 的本机 MCP（v0.10）：挂进这一个会话（`session/start`、`session/resume` 的 `config.mcpServers`，要 initialize 时
/// 申请 `sessionMcp`），不改 Muse 的设置；令牌在请求头里、走 stdio，不进命令行和环境变量。`optional`：连不上照样干活。
/// （2026-11-16 用本地假 MCP 核实过：建会话时 Muse 带着请求头连上来、列了工具，不花额度。）
fn session_config(task: &AgentTask) -> Option<Value> {
    let m = task.peck_mcp.as_ref()?;
    Some(json!({"mcpServers": {"peck": {
        "transport": "streamableHttp",
        "url": m.url,
        "headers": {"Authorization": format!("Bearer {}", m.token)},
        "mode": "optional",
    }}}))
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
    questions: HashMap<String, (String, Vec<Value>)>,
    finished: Option<TurnEndReason>,
    /// 转发 `/compact`（v1.1.7）：压缩那一条做完就算这一次完了。
    compacting: bool,
    /// 回合说完了、还没收（v1.1.9）：Muse 的后台命令跑完它会自己再开一回合汇报，以前回合一完就关进程，
    /// 后台命令跟着没了、也没人叫醒它（站主 2026-11-24 碰到的：「全量测试在跑，测完我就写 HANDOFF」然后就没了）。
    /// 现在先问 `task/list`：有后台命令在跑就等，等到它开下一回合接着收事件；没有就照常收。
    lingering: Option<Linger>,
}

/// 回合说完以后在等后台命令。
struct Linger {
    /// 正等着回应的那次 `task/list`。
    asked: Option<u64>,
    /// 头一次看到有后台命令在跑的时候（提示只写一次；最多等 [`BACKGROUND_WAIT`]）。
    since: Option<tokio::time::Instant>,
    /// 后台命令都没了的时候：Muse 该马上开下一回合，过了 [`WAKE_WAIT`] 还没开就收。
    clear: Option<tokio::time::Instant>,
}

/// 后台命令最多等多久（和 Claude Code 的后台子代理一样）。
const BACKGROUND_WAIT: Duration = Duration::from_secs(30 * 60);
/// 后台命令跑完以后等 Muse 开下一回合的时间。
const WAKE_WAIT: Duration = Duration::from_secs(4);
/// 等的时候隔多久再问一次 `task/list`。
const LINGER_TICK: Duration = Duration::from_secs(3);

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
            compacting: task.compact,
            lingering: None,
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
            // 等后台命令时隔几秒醒一下：再问一次、看等够了没有。
            let lingering = self.lingering.is_some();
            let tick = async move {
                if lingering {
                    tokio::time::sleep(LINGER_TICK).await
                } else {
                    std::future::pending().await
                }
            };
            tokio::select! {
                line = self.pipe.next() => match line {
                    Some(line) => self.handle(line).await,
                    // 回合已经说完、只是在等后台：进程没了就照做完收。
                    None if self.lingering.is_some() => self.finished = Some(TurnEndReason::Done),
                    None => {
                        let turn = self.turn_id.clone().unwrap_or_default();
                        emit_engine_died(&self.sink, "Muse", &turn, &self.pipe.stderr_tail());
                        self.finished = Some(TurnEndReason::Error);
                    }
                },
                Some(c) = control.recv() => {
                    // 回合已经说完、在等后台时按 STOP：不等了，照做完收（关进程时后台命令一起停）。
                    if matches!(c, Control::Interrupt) && self.lingering.is_some() {
                        self.finished = Some(TurnEndReason::Done);
                        continue;
                    }
                    if matches!(c, Control::Interrupt) && interrupt_deadline.is_none() {
                        interrupt_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                    }
                    self.control(c).await;
                }
                _ = tick => self.linger_tick().await,
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
            // `sessionMcp`：会话里挂 Peck 的本机 MCP（v0.10，见下面的 `config.mcpServers`）。
            json!({"clientInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"userInputDialogs": true, "requestedCapabilities": ["sessionMcp"]}}),
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
                if let Some(c) = session_config(task) {
                    p["config"] = c;
                }
                self.request("session/resume", p).await?
            }
            None => {
                let mut p = json!({"commandId": uuid_v7(), "workspaceRoots": [cwd], "approvalMode": mode});
                if let Some(model) = &task.model {
                    p["modelId"] = json!(model);
                }
                if let Some(c) = session_config(task) {
                    p["config"] = c;
                }
                self.request("session/start", p).await?
            }
        };
        self.session_id = session["session"]["sessionId"].as_str().map(str::to_string).or_else(|| task.resume.clone());
        let Some(session_id) = self.session_id.clone() else {
            return Err(GatewayError::new(ErrorKind::Engine, "Muse 没给会话号。"));
        };
        // 转发 `/compact`（v1.1.7）：`session/compact`（异步：accepted 以后看压缩那一条的结果；noop 就是没什么可压的）。不收重点。
        if task.compact {
            if task.resume.is_none() {
                return Err(GatewayError::new(ErrorKind::Engine, "这个任务还没有 Muse 的会话，没什么可压的。"));
            }
            if task.prompt.trim().trim_start_matches("/compact").trim() != "" {
                self.sink.emit(EventBody::Notice { level: crate::gateway::event::NoticeLevel::Info, text: "Muse 不接收重点，按它自己的方式压。".into() });
            }
            let turn_id = format!("compact-{}", task.run_id);
            self.turn_id = Some(turn_id.clone());
            self.sink.emit(EventBody::TurnStarted { turn_id: turn_id.clone() });
            let r = self.request("session/compact", json!({"commandId": uuid_v7(), "sessionId": session_id})).await?;
            if r["status"] == "noop" {
                let why = r["reason"].as_str().map(|w| format!("：{w}")).unwrap_or_default();
                self.sink.emit(EventBody::Notice { level: crate::gateway::event::NoticeLevel::Info, text: format!("Muse 说没什么可压的{why}") });
                self.sink.emit(EventBody::TurnCompleted { turn_id, reason: TurnEndReason::Done, error: None });
                self.finished = Some(TurnEndReason::Done);
            }
            return Ok(());
        }
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
        // 等后台时问的 `task/list` 回来了（v1.1.9）。
        if is_rpc_response(&raw) && self.lingering.as_ref().is_some_and(|l| l.asked.is_some_and(|id| raw["id"] == json!(id))) {
            self.linger_answer(&raw["result"]["tasks"]);
            return;
        }
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
                    // 问题原样留着：回答时照顺序一个个对上（v1.1.9）。
                    let asked = p["questions"].as_array().cloned().unwrap_or_default();
                    self.questions.insert(MuseConverter::input_id(input), (input.into(), asked));
                }
            }
            _ => {}
        }
        let emitted = self.converter.feed(&raw);
        for body in emit_all(&self.sink, emitted, Some(&raw)) {
            match body {
                EventBody::TurnCompleted { turn_id, reason, .. } => {
                    if self.compacting || self.turn_id.as_deref().is_none_or(|t| t == turn_id) {
                        if reason == TurnEndReason::Done && !self.compacting {
                            // 说完了：先看有没有后台命令还在跑。
                            self.lingering = Some(Linger { asked: None, since: None, clear: None });
                            self.ask_tasks().await;
                        } else {
                            self.finished = Some(reason);
                        }
                    }
                }
                // 等后台时 Muse 自己开了下一回合（后台命令跑完，它来汇报）：接着收这一回合。
                EventBody::TurnStarted { turn_id } if self.lingering.is_some() => {
                    self.turn_id = Some(turn_id);
                    self.lingering = None;
                }
                // 转发的 `/compact`：压缩那一条做完了就收（Muse 不一定另发回合结束）。
                EventBody::ContextCompacted { .. } if self.compacting && self.finished.is_none() => {
                    let failed = matches!(raw["params"]["item"]["outcome"].as_str(), Some("failed" | "cancelled"));
                    let (reason, error) = if failed { (TurnEndReason::Error, Some("Muse 没压成。".to_string())) } else { (TurnEndReason::Done, None) };
                    self.sink.emit(EventBody::TurnCompleted { turn_id: self.turn_id.clone().unwrap_or_default(), reason, error });
                    self.finished = Some(reason);
                }
                _ => {}
            }
        }
    }

    /// 问一次这个会话还有哪些活在跑（`task/list`，只读、不进模型）。发不出去就不等了。
    async fn ask_tasks(&mut self) {
        let Some(session) = self.session_id.clone() else {
            self.finished = Some(TurnEndReason::Done);
            return;
        };
        let id = self.next_id;
        if self.command("task/list", json!({"sessionId": session})).await.is_err() {
            self.finished = Some(TurnEndReason::Done);
            return;
        }
        if let Some(l) = self.lingering.as_mut() {
            l.asked = Some(id);
        }
    }

    /// `task/list` 的回应：还有后台命令在跑就接着等（头一次写一行），都没了就等 Muse 开下一回合。
    fn linger_answer(&mut self, tasks: &Value) {
        let running = tasks
            .as_array()
            .map(|t| t.iter().filter(|x| x["status"] == "inProgress" && x["executionMode"].as_str().unwrap_or("background") == "background").count())
            .unwrap_or(0);
        let Some(l) = self.lingering.as_mut() else { return };
        l.asked = None;
        if running > 0 {
            l.clear = None;
            if l.since.is_none() {
                l.since = Some(tokio::time::Instant::now());
                self.sink.emit(EventBody::Notice { level: NoticeLevel::Info, text: format!("等 Muse 的 {running} 条后台命令跑完") });
            }
        } else if l.since.is_none() {
            // 本来就没有后台命令：照常收。
            self.finished = Some(TurnEndReason::Done);
        } else if l.clear.is_none() {
            l.clear = Some(tokio::time::Instant::now());
        }
    }

    /// 等后台时隔几秒看一眼：等够了、或者后台都跑完了 Muse 也没再开回合，就收；不然再问一次。
    async fn linger_tick(&mut self) {
        let Some(l) = self.lingering.as_ref() else { return };
        let waited_out = l.since.is_some_and(|t| t.elapsed() >= BACKGROUND_WAIT);
        let no_wake = l.clear.is_some_and(|t| t.elapsed() >= WAKE_WAIT);
        if waited_out || no_wake {
            self.finished = Some(TurnEndReason::Done);
        } else if l.asked.is_none() || l.since.is_none() {
            // 上一次问的还没回（或者一直没回）：头一次都没回就别干等。
            if l.asked.is_some() && l.since.is_none() {
                self.finished = Some(TurnEndReason::Done);
            } else {
                self.ask_tasks().await;
            }
        }
    }

    async fn control(&mut self, c: Control) {
        let Some(session) = self.session_id.clone() else {
            return;
        };
        let (c, by) = c.with_origin();
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
                let Some((input, questions)) = self.questions.remove(&request_id) else {
                    return self.unknown(&request_id);
                };
                // 每个问题自己的答案（v1.1.9）。都按 `freeText` 给（Muse 最多收 500 字；多选的几项连成一句）。
                let answers: Vec<Value> = questions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, q)| {
                        let text: String = Control::picks(&answer, &each, i).join("、").chars().take(500).collect();
                        Some(json!({"questionId": q["id"].as_str()?, "freeText": text}))
                    })
                    .collect();
                let answer = crate::gateway::convert::ask_summary(&questions, &answer, &each);
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
            // 回合已经说完、在等后台命令时追加的一句（v1.1.9）：没有在跑的回合可插，直接开下一回合。
            Control::Append { text } if self.lingering.is_some() => {
                let _ = self
                    .command("turn/start", json!({"commandId": uuid_v7(), "sessionId": session, "input": [{"type": "text", "text": text}]}))
                    .await;
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
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
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
        assert_eq!(crate::gateway::engines::testing::without_estimates(got), want);
    }

    /// 回合说完了还有后台命令在跑（v1.1.9）：不收，等它跑完、Muse 自己再开一回合，接着收到那一回合结束。
    #[tokio::test]
    async fn a_turn_with_a_background_command_is_kept_until_muse_comes_back() {
        let lines = [
            json!({"id": 1, "result": {"serverInfo": {"name": "muse"}}}),
            json!({"method": "session/started", "params": {"session": {"sessionId": "s1", "modelId": "muse-spark-1.3"}}}),
            json!({"id": 2, "result": {"session": {"sessionId": "s1"}}}),
            json!({"id": 3, "result": {"turnId": "t1", "status": "accepted"}}),
            json!({"method": "turn/started", "params": {"turnId": "t1"}}),
            json!({"method": "turn/completed", "params": {"turnId": "t1", "terminal": "completed"}}),
            // 第 4 个请求是 Peck 问的 task/list：还有一条后台命令。
            json!({"id": 4, "result": {"tasks": [{"taskId": "k1", "kind": "tool", "status": "inProgress", "tool": "bash", "executionMode": "background", "command": "cargo test"}]}}),
            // 后台命令跑完，Muse 自己开了下一回合来汇报。
            json!({"method": "turn/started", "params": {"turnId": "t2"}}),
            json!({"method": "turn/completed", "params": {"turnId": "t2", "terminal": "completed"}}),
            json!({"id": 5, "result": {"tasks": []}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = auto_answer("muse", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = task();
        let outcome = tokio::time::timeout(Duration::from_secs(5), Session::new(pipe, &t, sink).run(t, rx)).await.expect("回放卡住了").unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let events = events.lock().unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.body.type_name()).filter(|k| k.starts_with("turn.") || *k == "notice").collect();
        assert_eq!(kinds, ["turn.started", "turn.completed", "notice", "turn.started", "turn.completed"]);
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::Notice { text, .. } if text == "等 Muse 的 1 条后台命令跑完")));
    }

    /// 等后台时按 STOP：不等了，照做完收（回合本来就说完了）。
    #[tokio::test]
    async fn stop_while_waiting_for_a_background_command_ends_the_task_as_done() {
        let lines = [
            json!({"id": 1, "result": {"serverInfo": {"name": "muse"}}}),
            json!({"id": 2, "result": {"session": {"sessionId": "s1"}}}),
            json!({"id": 3, "result": {"turnId": "t1", "status": "accepted"}}),
            json!({"method": "turn/started", "params": {"turnId": "t1"}}),
            json!({"method": "turn/completed", "params": {"turnId": "t1", "terminal": "completed"}}),
            json!({"id": 4, "result": {"tasks": [{"taskId": "k1", "kind": "tool", "status": "inProgress", "executionMode": "background"}]}}),
            // 之后 Muse 一直没动静；这一行要等 Peck 发第 99 个请求才放出来（永远不会）。
            json!({"id": 99, "result": {}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, rx) = mpsc::unbounded_channel();
        let (sink, events) = EventSink::memory("muse");
        let stopper = tx.clone();
        let seen = events.clone();
        tokio::spawn(async move {
            // 看到「等后台」那一行再按停。
            loop {
                tokio::time::sleep(Duration::from_millis(20)).await;
                if seen.lock().unwrap().iter().any(|e| matches!(e.body, EventBody::Notice { .. })) {
                    let _ = stopper.send(Control::Interrupt);
                    break;
                }
            }
        });
        let t = task();
        let outcome = tokio::time::timeout(Duration::from_secs(5), Session::new(pipe, &t, sink).run(t, rx)).await.expect("回放卡住了").unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        // 没有另发「已停止」的回合结束。
        assert_eq!(events.lock().unwrap().iter().filter(|e| e.body.type_name() == "turn.completed").count(), 1);
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

    #[tokio::test]
    async fn peck_mcp_is_attached_to_the_session_not_the_command_line() {
        let lines = [
            json!({"id": 1, "result": {"serverInfo": {"name": "muse"}, "grantedCapabilities": ["sessionMcp"]}}),
            json!({"id": 2, "result": {"session": {"sessionId": "s1"}}}),
            json!({"id": 3, "result": {"turnId": "t1", "status": "accepted"}}),
        ];
        let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
        let (tx, _rx) = mpsc::unbounded_channel();
        let (sink, _) = auto_answer("muse", tx, |_| ApprovalDecision::AllowOnce, "");
        let t = AgentTask { peck_mcp: Some(crate::gateway::PeckMcp { url: "http://127.0.0.1:9/mcp".into(), token: "tok".into() }), ..task() };
        let mut session = Session::new(pipe, &t, sink);
        session.start(&t).await.unwrap();
        let sent = session.pipe.sent();
        assert_eq!(sent[0]["params"]["capabilities"]["requestedCapabilities"], json!(["sessionMcp"]));
        let peck = &sent[2]["params"]["config"]["mcpServers"]["peck"];
        assert_eq!((peck["transport"].as_str(), peck["url"].as_str(), peck["mode"].as_str()), (Some("streamableHttp"), Some("http://127.0.0.1:9/mcp"), Some("optional")));
        assert_eq!(peck["headers"]["Authorization"], "Bearer tok");
        // 没有本机 MCP 就不带 config。
        assert!(session_config(&task()).is_none());
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
            resume: std::env::var("PECK_LIVE_RESUME").ok(),
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: std::env::var("PECK_LIVE_COMPACT").is_ok(),
            trusted: std::env::var("PECK_LIVE_TRUST").is_ok(),
        };
        let outcome = MuseBackend::new(setup).run(t, sink, rx).await;
        for e in events.lock().unwrap().iter() {
            println!("{}", serde_json::to_string(&e.body).unwrap().chars().take(220).collect::<String>());
        }
        println!("outcome: {outcome:?}");
    }

    /// 转发 `/compact`（v1.1.7）：`session/compact`；accepted 以后压缩那一条做完就收；noop 直接说没什么可压的。
    #[tokio::test]
    async fn compact_is_forwarded_and_ends_with_the_compaction_item() {
        let run = |status: &'static str, items: bool| async move {
            let mut lines = vec![
                json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
                json!({"jsonrpc": "2.0", "id": 2, "result": {"session": {"sessionId": "s1"}}}),
                json!({"jsonrpc": "2.0", "id": 3, "result": {"commandId": "x", "status": status, "reason": "上下文很小"}}),
            ];
            if items {
                lines.push(json!({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {"itemId": "k1", "kind": "compaction", "trigger": "manual"}}}));
                lines.push(json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {"itemId": "k1", "kind": "compaction", "trigger": "manual", "outcome": "compacted", "tokensBefore": 120000, "tokensAfter": 20000}}}));
                lines.push(json!({"jsonrpc": "2.0", "method": "session/contextUsage", "params": {"usedTokens": 21000, "windowTokens": 1000000, "pressure": "normal"}}));
            }
            let pipe = Pipe::script(&lines.iter().map(|l| l.to_string() + "\n").collect::<String>());
            let (tx, rx) = mpsc::unbounded_channel();
            let (sink, events) = auto_answer("muse", tx, |_| ApprovalDecision::AllowOnce, "");
            let t = AgentTask { resume: Some("s1".into()), compact: true, prompt: "/compact".into(), ..task() };
            let mut session = Session::new(pipe, &t, sink);
            session.start(&t).await.unwrap();
            let sent: Vec<String> = session.pipe.sent().iter().filter_map(|s| s["method"].as_str().map(str::to_string)).collect();
            let outcome = session.drive(&mut { rx }).await.unwrap();
            let bodies: Vec<EventBody> = events.lock().unwrap().iter().map(|e| e.body.clone()).collect();
            (outcome, sent, bodies)
        };
        let (outcome, sent, bodies) = run("accepted", true).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(sent.contains(&"session/compact".to_string()) && !sent.contains(&"turn/start".to_string()), "{sent:?}");
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ContextCompacted { before: 120000, after: 20000, .. })));
        assert!(matches!(bodies.iter().rev().find(|b| matches!(b, EventBody::TurnCompleted { .. })), Some(EventBody::TurnCompleted { reason: TurnEndReason::Done, .. })));
        let (outcome, _, bodies) = run("noop", false).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(bodies.iter().any(|b| matches!(b, EventBody::Notice { text, .. } if text.contains("没什么可压的：上下文很小"))));
    }

}
