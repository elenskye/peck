//! 假 Gateway：云端开发、原生的假数据模式和测试都用它，不发真请求。
//!
//! - [`ScriptedModel`]：照剧本出字、出思考、出工具调用的假模型（实现 [`ModelProvider`]）。
//!   默认剧本 [`ScriptedModel::peck_demo`]：读目录 → 读文件 → 搜 → 改 → 跑命令 → 清单 → 问一句 → 收尾。
//! - [`ReplayBackend`]：回放夹具里录好的统一事件的假引擎（实现 [`AgentBackend`]）。
//!   碰到 `approval.requested` / `user_input.requested` 就停下等控制通道，和真引擎一样。

use super::caps::AgentCaps;
use super::catalog::catalog;
use super::event::{
    ApprovalDecision, EventBody, OpaqueReasoning, Origin, ResolvedBy, TokenUsage, ToolStatus,
    TurnEndReason, Wire,
};
use super::convert::Emitted;
use super::log::EventSink;
use super::{
    AgentBackend, AgentTask, BoxFuture, Control, GatewayError, Message, ModelEvent, ModelProvider,
    ModelRequest, ModelResponse, Part, Role, RunOutcome, StopReason,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

// ───────────────────────── 假模型 ─────────────────────────

/// 剧本里的一步：模型这一次回什么。
#[derive(Debug, Clone)]
pub struct ScriptTurn {
    pub reasoning: Option<String>,
    pub text: Option<String>,
    /// (工具名, 参数)
    pub tool_calls: Vec<(String, Value)>,
    pub usage: TokenUsage,
}

pub struct ScriptedModel {
    model: String,
    turns: Mutex<std::collections::VecDeque<ScriptTurn>>,
    /// 每段字之间停多久（原生假数据模式看得见在流；测试为 0）。
    pace: Duration,
    /// 收到的请求，测试用来查调用方发了什么。
    pub requests: Arc<Mutex<Vec<ModelRequest>>>,
}

impl ScriptedModel {
    pub fn new(model: impl Into<String>, turns: Vec<ScriptTurn>, pace: Duration) -> Self {
        Self {
            model: model.into(),
            turns: Mutex::new(turns.into()),
            pace,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Peck 循环的演示剧本，工具名照 Phase 6 的九个工具。
    pub fn peck_demo(pace: Duration) -> Self {
        let usage = |input: u64, output: u64, cache_read: u64| TokenUsage {
            input: Some(input),
            output: Some(output),
            cache_read: Some(cache_read),
            cache_write: Some(0),
            reasoning: Some(output / 3),
            ..Default::default()
        };
        let call = |name: &str, input: Value| (name.to_string(), input);
        let turn = |reasoning: &str, text: &str, calls: Vec<(String, Value)>, u: TokenUsage| ScriptTurn {
            reasoning: (!reasoning.is_empty()).then(|| reasoning.to_string()),
            text: (!text.is_empty()).then(|| text.to_string()),
            tool_calls: calls,
            usage: u,
        };
        Self::new(
            "deepseek/deepseek-flash",
            vec![
                turn(
                    "先看项目里有什么。",
                    "我先看一下项目结构。",
                    vec![call("list_directory", json!({"path": ".", "depth": 2}))],
                    usage(4_200, 120, 0),
                ),
                turn(
                    "",
                    "",
                    vec![call("read_file", json!({"path": "src/App.tsx", "start_line": 1, "line_count": 200}))],
                    usage(900, 60, 4_200),
                ),
                turn(
                    "标题在哪里定义？搜一下。",
                    "",
                    vec![call("search_text", json!({"pattern": "<h1", "path": "src", "mode": "content"}))],
                    usage(1_400, 80, 5_100),
                ),
                turn(
                    "",
                    "找到了，把标题改成「待办」。",
                    vec![call(
                        "write_patch",
                        json!({"edits": [{"path": "src/App.tsx", "old_text": "<h1>Todo</h1>", "new_text": "<h1>待办</h1>"}]}),
                    )],
                    usage(600, 140, 6_500),
                ),
                turn(
                    "",
                    "",
                    vec![call("bash_run", json!({"command": "npm test", "timeout_sec": 120}))],
                    usage(300, 40, 7_100),
                ),
                turn(
                    "",
                    "",
                    vec![call(
                        "update_plan",
                        json!({"steps": [
                            {"text": "找到标题", "status": "completed"},
                            {"text": "改成中文", "status": "completed"},
                            {"text": "跑测试", "status": "completed"},
                            {"text": "确认要不要也改副标题", "status": "in_progress"}
                        ]}),
                    )],
                    usage(900, 90, 7_400),
                ),
                turn(
                    "",
                    "",
                    vec![call(
                        "ask_user",
                        json!({"question": "副标题也要改成中文吗？", "options": ["一起改", "先不改"]}),
                    )],
                    usage(200, 50, 8_300),
                ),
                turn(
                    "站主说先不改，收尾。",
                    "改好了：\n\n- `src/App.tsx` 的标题改成「待办」\n- `npm test` 通过\n\n副标题照你说的先不动。",
                    vec![],
                    usage(150, 160, 8_500),
                ),
            ],
            pace,
        )
    }

    fn origin(&self) -> Origin {
        let spec = catalog().model(&self.model);
        Origin {
            provider: spec.map(|m| m.provider.clone()).unwrap_or_else(|| "mock".into()),
            wire: Wire::Chat,
            model: spec
                .map(|m| m.api_model.clone())
                .unwrap_or_else(|| self.model.clone()),
        }
    }
}

impl ModelProvider for ScriptedModel {
    fn id(&self) -> &str {
        "mock"
    }

    fn wire(&self) -> Wire {
        Wire::Chat
    }

    fn complete(
        &self,
        request: ModelRequest,
        events: mpsc::UnboundedSender<ModelEvent>,
        mut cancel: watch::Receiver<bool>,
    ) -> BoxFuture<Result<ModelResponse, GatewayError>> {
        self.requests.lock().unwrap().push(request.clone());
        let next = self.turns.lock().unwrap().pop_front();
        let pace = self.pace;
        let origin = self.origin();
        let model = self.model.clone();
        Box::pin(async move {
            let turn = next.unwrap_or(ScriptTurn {
                reasoning: None,
                text: Some("（假模型的剧本演完了。）".into()),
                tool_calls: vec![],
                usage: TokenUsage::default(),
            });
            let mut parts = Vec::new();
            let step = |cancel: &mut watch::Receiver<bool>| {
                let cancelled = *cancel.borrow();
                async move {
                    if cancelled {
                        return Err(GatewayError::cancelled());
                    }
                    if !pace.is_zero() {
                        tokio::time::sleep(pace).await;
                    }
                    Ok(())
                }
            };
            if let Some(reasoning) = &turn.reasoning {
                for chunk in chunks(reasoning) {
                    step(&mut cancel).await?;
                    let _ = events.send(ModelEvent::ReasoningDelta { text: chunk });
                }
                parts.push(Part::Reasoning {
                    text: reasoning.clone(),
                    opaque: Some(OpaqueReasoning {
                        origin: origin.clone(),
                        data: json!({"reasoning_content": reasoning}),
                    }),
                });
            }
            if let Some(text) = &turn.text {
                for chunk in chunks(text) {
                    step(&mut cancel).await?;
                    let _ = events.send(ModelEvent::TextDelta { text: chunk });
                }
                parts.push(Part::Text { text: text.clone() });
            }
            for (i, (name, input)) in turn.tool_calls.iter().enumerate() {
                step(&mut cancel).await?;
                let call_id = format!("mock-call-{}-{i}", uuid::Uuid::new_v4().simple());
                let _ = events.send(ModelEvent::ToolCallStarted {
                    call_id: call_id.clone(),
                    name: name.clone(),
                });
                parts.push(Part::ToolCall {
                    call_id,
                    name: name.clone(),
                    input: input.clone(),
                    signature: None,
                });
            }
            let cost = catalog()
                .model(&model)
                .map(|m| m.cost(&turn.usage, None))
                .unwrap_or_else(super::event::Cost::unknown);
            Ok(ModelResponse {
                stop: if turn.tool_calls.is_empty() {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                },
                message: Message {
                    role: Role::Assistant,
                    parts,
                },
                usage: turn.usage,
                cost,
                response_model: Some(origin.model),
            })
        })
    }
}

/// 按字切成几段（中文按字符，不切坏多字节）。
fn chunks(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(6).map(|c| c.iter().collect()).collect()
}

// ───────────────────────── 假引擎 ─────────────────────────

/// 回放一份统一事件（夹具的 `events.jsonl`，一行一条 [`Emitted`]）。
pub struct ReplayBackend {
    engine: String,
    events: Vec<Emitted>,
    pace: Duration,
}

impl ReplayBackend {
    pub fn new(engine: impl Into<String>, events: Vec<Emitted>, pace: Duration) -> Self {
        Self {
            engine: engine.into(),
            events,
            pace,
        }
    }

    /// 读 `events.jsonl`：空行跳过，读不懂的行报错（夹具是自己录的，格式错了要早发现）。
    pub fn from_jsonl(engine: impl Into<String>, jsonl: &str, pace: Duration) -> Result<Self, String> {
        let events = parse_events_jsonl(jsonl)?;
        Ok(Self::new(engine, events, pace))
    }

    /// 仓库里录好的夹具：`codex`、`claude-code`。
    pub fn fixture(engine: &str, pace: Duration) -> Option<Self> {
        let jsonl = match engine {
            "codex" => include_str!("../../tests/fixtures/gateway/codex-small-task/events.jsonl"),
            "claude-code" => {
                include_str!("../../tests/fixtures/gateway/claude-code-small-task/events.jsonl")
            }
            _ => return None,
        };
        Self::from_jsonl(engine, jsonl, pace).ok()
    }
}

pub fn parse_events_jsonl(jsonl: &str) -> Result<Vec<Emitted>, String> {
    jsonl
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("第 {} 行：{e}", i + 1)))
        .collect()
}

fn call_id_of(body: &EventBody) -> Option<&str> {
    match body {
        EventBody::ToolCallStarted { call_id, .. }
        | EventBody::ToolCallOutputDelta { call_id, .. }
        | EventBody::ToolCallCompleted { call_id, .. } => Some(call_id),
        _ => None,
    }
}

impl AgentBackend for ReplayBackend {
    fn id(&self) -> &str {
        &self.engine
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::for_engine(&self.engine)
    }

    fn run(
        &self,
        task: AgentTask,
        sink: EventSink,
        mut control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let events = self.events.clone();
        let pace = self.pace;
        let _ = task;
        Box::pin(async move {
            // 早到的控制消息（审批先于请求到达）先放着，等到要用时再取。
            let mut early: std::collections::VecDeque<Control> = Default::default();
            let mut denied_calls: Vec<String> = Vec::new();
            let mut skip_resolved: Vec<String> = Vec::new();
            let mut session_ref = None;
            let mut last_turn = String::from("turn-1");
            for Emitted { subagent_id, body } in events {
                let sink = match &subagent_id {
                    Some(id) => sink.for_subagent(id.clone()),
                    None => sink.clone(),
                };
                if !pace.is_zero() {
                    tokio::time::sleep(pace).await;
                }
                // 站主打断：收尾。别的控制消息留着。
                while let Ok(msg) = control.try_recv() {
                    early.push_back(msg);
                }
                if early.iter().any(|m| *m == Control::Interrupt) {
                    sink.emit(EventBody::TurnCompleted {
                        turn_id: last_turn,
                        reason: TurnEndReason::Interrupted,
                        error: None,
                    });
                    return Ok(RunOutcome {
                        reason: TurnEndReason::Interrupted,
                        session_ref,
                    });
                }
                if call_id_of(&body).is_some_and(|id| denied_calls.iter().any(|d| d == id)) {
                    continue;
                }
                match &body {
                    EventBody::SessionStarted { session_ref: r, .. } => session_ref = r.clone(),
                    EventBody::TurnStarted { turn_id } => last_turn = turn_id.clone(),
                    EventBody::ApprovalResolved { request_id, .. }
                        if skip_resolved.contains(request_id) =>
                    {
                        continue
                    }
                    _ => {}
                }
                sink.emit(body.clone());
                match body {
                    EventBody::ApprovalRequested {
                        request_id,
                        call_id,
                        ..
                    } => {
                        let decision = loop {
                            let next = match early.pop_front() {
                                Some(m) => Some(m),
                                None => control.recv().await,
                            };
                            match next {
                                Some(Control::Approve {
                                    request_id: id,
                                    decision,
                                }) if id == request_id => break decision,
                                Some(Control::Interrupt) | None => {
                                    sink.emit(EventBody::TurnCompleted {
                                        turn_id: last_turn,
                                        reason: TurnEndReason::Interrupted,
                                        error: None,
                                    });
                                    return Ok(RunOutcome {
                                        reason: TurnEndReason::Interrupted,
                                        session_ref,
                                    });
                                }
                                _ => {}
                            }
                        };
                        sink.emit(EventBody::ApprovalResolved {
                            request_id: request_id.clone(),
                            decision,
                            by: ResolvedBy::User,
                        });
                        skip_resolved.push(request_id);
                        if !decision.allows() {
                            if let Some(call_id) = call_id {
                                sink.emit(EventBody::ToolCallCompleted {
                                    call_id: call_id.clone(),
                                    status: ToolStatus::Denied,
                                    output: Some("站主拒绝了。".into()),
                                    exit_code: None,
                                    diff: vec![],
                                    duration_ms: None,
                                });
                                denied_calls.push(call_id);
                            }
                        }
                    }
                    EventBody::UserInputRequested { request_id, .. } => {
                        let answer = loop {
                            let next = match early.pop_front() {
                                Some(m) => Some(m),
                                None => control.recv().await,
                            };
                            match next {
                                Some(Control::Answer {
                                    request_id: id,
                                    answer,
                                }) if id == request_id => break answer,
                                Some(Control::Interrupt) | None => {
                                    return Ok(RunOutcome {
                                        reason: TurnEndReason::Interrupted,
                                        session_ref,
                                    })
                                }
                                _ => {}
                            }
                        };
                        sink.emit(EventBody::UserInputResolved { request_id, answer });
                    }
                    EventBody::TurnCompleted { reason, .. } => {
                        return Ok(RunOutcome {
                            reason,
                            session_ref,
                        })
                    }
                    _ => {}
                }
            }
            Ok(RunOutcome {
                reason: TurnEndReason::Done,
                session_ref,
            })
        })
    }
}

/// 发一个审批回答（测试和假数据用的小工具）。
pub fn approve(request_id: &str, decision: ApprovalDecision) -> Control {
    Control::Approve {
        request_id: request_id.into(),
        decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::{PermissionLevel, ToolKind};

    fn task() -> AgentTask {
        AgentTask {
            run_id: "run-1".into(),
            prompt: "改标题".into(),
            cwd: "/tmp/ws".into(),
            permission: PermissionLevel::Edit,
            model: None,
            effort: None,
            resume: None,
            peck_mcp: None,
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            model: "deepseek/deepseek-flash".into(),
            system: vec![],
            messages: vec![],
            tools: vec![],
            thinking: true,
            effort: None,
            max_output_tokens: 1000,
        }
    }

    #[tokio::test]
    async fn scripted_model_walks_the_demo_tools_in_order() {
        let model = ScriptedModel::peck_demo(Duration::ZERO);
        let mut names = Vec::new();
        let (_cancel_tx, cancel) = watch::channel(false);
        loop {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let response = model.complete(request(), tx, cancel.clone()).await.unwrap();
            let mut text = String::new();
            while let Ok(ModelEvent::TextDelta { text: t }) | Ok(ModelEvent::ReasoningDelta { text: t }) = rx.try_recv() {
                text.push_str(&t);
            }
            assert_eq!(response.cost.source, crate::gateway::event::CostSource::Priced);
            for part in &response.message.parts {
                if let Part::ToolCall { name, .. } = part {
                    names.push(name.clone());
                }
            }
            if response.stop == StopReason::EndTurn {
                break;
            }
        }
        assert_eq!(
            names,
            ["list_directory", "read_file", "search_text", "write_patch", "bash_run", "update_plan", "ask_user"]
        );
        assert_eq!(model.requests.lock().unwrap().len(), 8);
    }

    #[tokio::test]
    async fn scripted_model_stops_when_cancelled() {
        let model = ScriptedModel::peck_demo(Duration::ZERO);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_cancel_tx, cancel) = watch::channel(true);
        let error = model.complete(request(), tx, cancel).await.unwrap_err();
        assert_eq!(error.kind, crate::gateway::ErrorKind::Cancelled);
    }

    fn approval_script() -> Vec<EventBody> {
        vec![
            EventBody::TurnStarted { turn_id: "t1".into() },
            EventBody::ToolCallStarted {
                call_id: "c1".into(),
                name: "Bash".into(),
                kind: ToolKind::Run,
                title: "跑 rm a.txt".into(),
                input: json!({"command": "rm a.txt"}),
            },
            EventBody::ApprovalRequested {
                request_id: "r1".into(),
                call_id: Some("c1".into()),
                kind: ToolKind::Run,
                title: "删 a.txt".into(),
                detail: Some("rm a.txt".into()),
                high_risk: true,
                options: vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce],
            },
            EventBody::ApprovalResolved {
                request_id: "r1".into(),
                decision: ApprovalDecision::AllowOnce,
                by: ResolvedBy::User,
            },
            EventBody::ToolCallCompleted {
                call_id: "c1".into(),
                status: ToolStatus::Ok,
                output: Some(String::new()),
                exit_code: Some(0),
                diff: vec![],
                duration_ms: None,
            },
            EventBody::TurnCompleted {
                turn_id: "t1".into(),
                reason: TurnEndReason::Done,
                error: None,
            },
        ]
    }

    #[tokio::test]
    async fn replay_waits_for_approval_and_honours_a_denial() {
        let backend = ReplayBackend::new("codex", approval_script().into_iter().map(Into::into).collect(), Duration::ZERO);
        let (sink, events) = EventSink::memory("codex");
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(approve("r1", ApprovalDecision::DenyOnce)).unwrap();
        let outcome = backend.run(task(), sink, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let events = events.lock().unwrap();
        let types: Vec<&str> = events.iter().map(|e| e.body.type_name()).collect();
        assert_eq!(
            types,
            ["turn.started", "tool_call.started", "approval.requested", "approval.resolved", "tool_call.completed", "turn.completed"]
        );
        assert!(matches!(
            events[4].body,
            EventBody::ToolCallCompleted { status: ToolStatus::Denied, .. }
        ));
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn replay_stops_on_interrupt_while_waiting() {
        let backend = ReplayBackend::new("codex", approval_script().into_iter().map(Into::into).collect(), Duration::ZERO);
        let (sink, events) = EventSink::memory("codex");
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            tx.send(Control::Interrupt).unwrap();
        });
        let outcome = backend.run(task(), sink, rx).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::Interrupted);
        assert_eq!(events.lock().unwrap().last().unwrap().body.type_name(), "turn.completed");
    }
}
