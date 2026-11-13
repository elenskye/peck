//! Peck 循环：一个回合里反复「问模型 → 跑它要的工具 → 把结果放回去」，直到模型不再要工具。
//!
//! - 模型经 Gateway 的 [`ModelProvider`] 调（真 API 或假模型），对话用和供应商无关的 `Message` / `Part`；
//!   思考的私有数据原样挂在 `Part::Reasoning.opaque` 上，下一轮原样放回（换了供应商由 provider 丢掉）。
//! - 事件经 `EventSink` 发：和 CLI 引擎同一套（`session.started` 打头、`turn.completed` 收尾）。
//! - 上限：默认 40 轮；上下文用到模型窗口的 85% 先停下（压缩是 v0.6）；花费上限到了在下一次请求前停；
//!   供应商没报用量也停。失败、取消、重开都不自动重发。
//! - 控制通道：审批、回答（ASK）、追加一句（下一次请求带上）、打断（停下当前请求或命令）。

use super::permission::{self, Ask};
use super::spill;
use super::tools::{self, bash, ToolContext};
use crate::gateway::caps::AgentCaps;
use crate::gateway::catalog::{self, Thinking};
use crate::gateway::event::{
    ApprovalDecision, EventBody, NoticeLevel, PermissionLevel, PlanStatus, PlanStep, ResolvedBy, ToolKind, ToolStatus,
    TurnEndReason, UsageScope,
};
use crate::gateway::log::EventSink;
use crate::gateway::{
    AgentBackend, AgentTask, BoxFuture, Control, ErrorKind, GatewayError, Message, ModelEvent, ModelProvider,
    ModelRequest, Part, Role, RunOutcome, StopReason, SystemBlock,
};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{mpsc, watch};

pub const DEFAULT_MAX_ROUNDS: u32 = 40;
pub const CONTEXT_STOP_RATIO: f64 = 0.85;

/// 花费上限：这个月已经花了多少，上限多少（美元）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub cap_usd: f64,
    pub spent_usd: f64,
}

/// 这个月 Peck 循环和 ONE SHOT 一共花了多少（从事件日志现算，包括别的任务同时花的）。读不到为空。
pub type SpendProbe = std::sync::Arc<dyn Fn() -> Option<f64> + Send + Sync>;

#[derive(Clone)]
pub struct LoopConfig {
    /// 目录里的模型 id（`deepseek/deepseek-flash`）。
    pub model_id: String,
    pub max_rounds: u32,
    pub budget: Option<Budget>,
    /// 每次请求前现算这个月花了多少；空着就用开工时的 `budget.spent_usd` 加上这个任务自己花的。
    pub live_spend: Option<SpendProbe>,
    /// ONE SHOT：只读回答，只给读的工具，不弹清单和提问。
    pub oneshot: bool,
    /// 这个项目「总是允许」过的类（`install-deps`、`long-running`）。
    pub allowed: HashSet<String>,
    /// 命令输出落盘的目录（这个任务一个）。
    pub spill_dir: PathBuf,
    /// 命令的 HOME、临时目录放哪（每个项目一个）。
    pub runtime_dir: PathBuf,
    /// 跑命令的环境。
    pub env: Vec<(String, String)>,
    /// 续接用的会话号（放进 `session.started`）。
    pub session_ref: Option<String>,
}

type AllowHook = Arc<dyn Fn(&str) + Send + Sync>;

/// Peck 自己的循环 + 一个模型。`conversation` 进来是之前的对话，跑完是加上这一回合的。
pub struct PeckBackend {
    provider: Arc<dyn ModelProvider>,
    config: LoopConfig,
    pub conversation: Arc<Mutex<Vec<Message>>>,
    on_allow_always: Option<AllowHook>,
}

impl PeckBackend {
    pub fn new(provider: Arc<dyn ModelProvider>, config: LoopConfig, history: Vec<Message>) -> Self {
        Self {
            provider,
            config,
            conversation: Arc::new(Mutex::new(history)),
            on_allow_always: None,
        }
    }

    /// 站主点了「这个项目总是允许」：记下来（存进数据库由调用方做）。
    pub fn on_allow_always(mut self, hook: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_allow_always = Some(Arc::new(hook));
        self
    }
}

impl AgentBackend for PeckBackend {
    fn id(&self) -> &str {
        "peck"
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::peck_loop()
    }

    fn run(&self, task: AgentTask, sink: EventSink, control: mpsc::UnboundedReceiver<Control>) -> BoxFuture<Result<RunOutcome, GatewayError>> {
        let provider = self.provider.clone();
        let config = self.config.clone();
        let conversation = self.conversation.clone();
        let hook = self.on_allow_always.clone();
        Box::pin(async move {
            let level = if config.oneshot { PermissionLevel::Plan } else { task.permission };
            let ctx = ToolContext::new(&task.cwd, level, config.spill_dir.clone(), config.runtime_dir.clone(), config.env.clone())
                .map_err(|e| GatewayError::new(ErrorKind::Internal, e))?;
            let (cancel_tx, cancel_rx) = watch::channel(false);
            let history = conversation.lock().map(|c| c.clone()).unwrap_or_default();
            let mut run = Run {
                provider,
                allowed: config.allowed.clone(),
                config,
                task,
                level,
                ctx: Arc::new(ctx),
                sink,
                ctl: Ctl::new(control),
                cancel_tx,
                cancel_rx,
                messages: history,
                hook,
                approvals: 0,
                cost_usd: 0.0,
                warned_unpriced: false,
            };
            let outcome = run.drive().await;
            if let Ok(mut c) = conversation.lock() {
                *c = run.messages.clone();
            }
            Ok(outcome)
        })
    }
}

/// 控制通道：收审批、回答；追加的话攒着下一次请求带上；打断记一笔。
struct Ctl {
    rx: mpsc::UnboundedReceiver<Control>,
    steer: Vec<String>,
    interrupted: bool,
    closed: bool,
    stray: Vec<Control>,
}

impl Ctl {
    fn new(rx: mpsc::UnboundedReceiver<Control>) -> Self {
        Self { rx, steer: vec![], interrupted: false, closed: false, stray: vec![] }
    }

    /// 处理一条；审批、回答原样还回去。
    fn take(&mut self, c: Control) -> Option<Control> {
        match c {
            Control::Append { text } => {
                if !text.trim().is_empty() {
                    self.steer.push(text);
                }
                None
            }
            Control::Interrupt => {
                self.interrupted = true;
                None
            }
            other => Some(other),
        }
    }

    fn drain(&mut self) {
        while let Ok(c) = self.rx.try_recv() {
            if let Some(other) = self.take(c) {
                self.stray.push(other);
            }
        }
    }

    /// 等到站主打断（收到的追加照攒）。通道断了就一直等下去（没人能打断了）。
    async fn until_interrupt(&mut self) {
        loop {
            if self.interrupted {
                return;
            }
            if self.closed {
                std::future::pending::<()>().await;
            }
            match self.rx.recv().await {
                Some(c) => {
                    if let Some(other) = self.take(c) {
                        self.stray.push(other);
                    }
                }
                None => self.closed = true,
            }
        }
    }

    /// 等某个请求的回答。被打断或者通道断了返回 `None`。
    async fn wait_for(&mut self, id: &str) -> Option<Control> {
        let matches = |c: &Control| match c {
            Control::Approve { request_id, .. } | Control::Answer { request_id, .. } => request_id == id,
            _ => false,
        };
        if let Some(pos) = self.stray.iter().position(matches) {
            return Some(self.stray.remove(pos));
        }
        loop {
            if self.interrupted || self.closed {
                return None;
            }
            match self.rx.recv().await {
                Some(c) => match self.take(c) {
                    Some(c) if matches(&c) => return Some(c),
                    Some(other) => self.stray.push(other),
                    None => {}
                },
                None => self.closed = true,
            }
        }
    }
}

struct Run {
    provider: Arc<dyn ModelProvider>,
    config: LoopConfig,
    task: AgentTask,
    level: PermissionLevel,
    ctx: Arc<ToolContext>,
    sink: EventSink,
    ctl: Ctl,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,
    messages: Vec<Message>,
    allowed: HashSet<String>,
    hook: Option<AllowHook>,
    approvals: u32,
    cost_usd: f64,
    warned_unpriced: bool,
}

/// 一回合怎么结束的。
struct End {
    reason: TurnEndReason,
    error: Option<String>,
}

impl End {
    fn done() -> Self {
        Self { reason: TurnEndReason::Done, error: None }
    }
    fn limit(why: impl Into<String>) -> Self {
        Self { reason: TurnEndReason::Limit, error: Some(why.into()) }
    }
    fn interrupted() -> Self {
        Self { reason: TurnEndReason::Interrupted, error: None }
    }
}

fn system_prompt(task: &AgentTask, level: PermissionLevel, oneshot: bool) -> String {
    let today = time::OffsetDateTime::now_utc().date();
    let mode = if oneshot {
        "ONE SHOT (read-only): answer the user's question by reading the project. Do not change any files and do not run commands that write."
    } else {
        match level {
            PermissionLevel::Plan => "PLAN (read-only): investigate and propose a plan. You cannot edit files; commands run in a read-only sandbox.",
            PermissionLevel::Edit => "EDIT: you may edit files and run commands. Installing dependencies or starting long-running servers asks the user first.",
            PermissionLevel::Auto => "AUTO: you may edit files and run commands without asking, except dangerous ones.",
        }
    };
    format!(
        "You are Peck, a coding agent working in the user's project on their Mac.\n\
         Project root: {root} (tool paths are relative to it). Today: {today}.\n\
         Permission: {mode}\n\
         Deleting files, git push or rewriting history, writing outside the project and changing system settings always need the user's approval; if they say no, find another way or explain.\n\n\
         How to work:\n\
         - Look before you change: list, search and read the relevant files first.\n\
         - Edit with write_patch (exact old_text → new_text); use write_file for new files or full rewrites.\n\
         - For multi-step work keep a short checklist with update_plan (one step in_progress at a time).\n\
         - Verify with bash_run (tests, build, lint) when it makes sense. A failing command is evidence, not success.\n\
         - Use ask_user only when you cannot continue without the user's decision.\n\
         - Answer in the user's language (Chinese if they write Chinese), in Markdown, briefly: what you changed, how you checked it, anything left.",
        root = task.cwd.display(),
    )
}

impl Run {
    fn emit(&self, body: EventBody) {
        self.sink.emit(body);
    }

    fn model_spec(&self) -> Option<&'static catalog::ModelSpec> {
        catalog::catalog().model(&self.config.model_id)
    }

    /// 把一条用户消息接到对话末尾（上一条也是用户的就并进去，保证一问一答交替）。
    fn push_user(&mut self, parts: Vec<Part>) {
        if parts.is_empty() {
            return;
        }
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.parts.extend(parts),
            _ => self.messages.push(Message { role: Role::User, parts }),
        }
    }

    fn take_steer(&mut self) -> Vec<Part> {
        self.ctl.drain();
        std::mem::take(&mut self.ctl.steer)
            .into_iter()
            .map(|text| Part::Text { text: format!("（站主追加）{text}") })
            .collect()
    }

    async fn drive(&mut self) -> RunOutcome {
        let names = tools::names_for(self.level, self.config.oneshot);
        let turn_id = format!("turn-{}", self.task.run_id);
        self.emit(EventBody::SessionStarted {
            engine: "peck".into(),
            model: Some(self.config.model_id.clone()),
            caps: AgentCaps::peck_loop(),
            tools: names.iter().map(|s| s.to_string()).collect(),
            mcp: vec![],
            session_ref: self.config.session_ref.clone(),
            version: None,
        });
        self.emit(EventBody::TurnStarted { turn_id: turn_id.clone() });
        self.push_user(vec![Part::Text { text: self.task.prompt.clone() }]);
        let end = self.rounds(&names).await;
        self.emit(EventBody::TurnCompleted { turn_id, reason: end.reason, error: end.error });
        RunOutcome { reason: end.reason, session_ref: self.config.session_ref.clone() }
    }

    async fn rounds(&mut self, names: &[&'static str]) -> End {
        let specs = tools::specs_for(names);
        let system = vec![SystemBlock { text: system_prompt(&self.task, self.level, self.config.oneshot), cache: true }];
        let spec = self.model_spec();
        let window = spec.and_then(|m| m.context_window);
        let max_output = spec.and_then(|m| m.max_output).unwrap_or(8192).clamp(1024, 32_000) as u32;
        let effort = self.task.effort.clone().filter(|e| spec.is_some_and(|m| m.efforts.contains(e)));
        let thinking = match spec.map(|m| m.thinking) {
            Some(Thinking::Always) => true,
            Some(Thinking::Switch) => effort.is_some(),
            _ => false,
        };
        // 上一次请求的提示 + 输出，加上这次新放进去的东西，估下一次的上下文。
        let mut context_estimate: u64 = 0;
        let mut round = 0u32;
        loop {
            self.ctl.drain();
            if self.ctl.interrupted {
                return End::interrupted();
            }
            if round >= self.config.max_rounds {
                return End::limit(format!("到了 {} 轮的上限，先停下。文件和过程都留着，接着说一句就能继续。", self.config.max_rounds));
            }
            if let (Some(window), true) = (window, context_estimate > 0) {
                let ratio = context_estimate as f64 / window as f64;
                if ratio >= CONTEXT_STOP_RATIO {
                    return End::limit(format!(
                        "上下文用到了模型窗口的 {:.0}%，先停下（自动压缩在 v0.6）。换一个新任务接着做，或者把要点再说一遍。",
                        ratio * 100.0
                    ));
                }
            }
            if let Some(b) = self.config.budget {
                // 同时跑的任务（别的项目、ONE SHOT）也在花：每次现算，不用开工时那个数。
                let spent = self
                    .config
                    .live_spend
                    .as_ref()
                    .and_then(|probe| probe())
                    .map(|month| month.max(b.spent_usd + self.cost_usd))
                    .unwrap_or(b.spent_usd + self.cost_usd);
                if round == 0 && spec.is_some_and(|m| m.price.is_none()) {
                    return End::limit(
                        "这个模型的价格表里没有价格，花了多少算不出来；设了花费上限时不发。换一个有价格的模型，或者在设置里把花费上限清空。",
                    );
                }
                if spent >= b.cap_usd {
                    return End::limit(format!(
                        "这个月的 API 花费到了上限（${:.2}，已用 ${spent:.2}），没发下一次请求。在设置里调高上限再接着做。",
                        b.cap_usd
                    ));
                }
            }
            round += 1;
            let request = ModelRequest {
                model: self.config.model_id.clone(),
                system: system.clone(),
                messages: self.messages.clone(),
                tools: specs.clone(),
                thinking,
                effort: effort.clone(),
                max_output_tokens: max_output,
            };
            let response = match self.ask_model(request, round).await {
                Ok(r) => r,
                Err(e) if e.kind == ErrorKind::Cancelled || self.ctl.interrupted => return End::interrupted(),
                Err(e) => {
                    self.emit(EventBody::Error { message: e.message.clone(), fatal: true });
                    return End { reason: TurnEndReason::Error, error: Some(e.message) };
                }
            };
            // 用量、花费
            self.emit(EventBody::UsageUpdated { usage: response.usage.clone(), cost: response.cost.clone(), scope: UsageScope::Turn });
            if let Some(usd) = response.cost.usd {
                self.cost_usd += usd;
            } else if self.config.budget.is_some() && !self.warned_unpriced {
                self.warned_unpriced = true;
                self.emit(EventBody::Notice {
                    level: NoticeLevel::Warn,
                    text: "这一次的花费算不出来（供应商没报用量，或者价格表里没有这个模型），花费上限管不到它。".into(),
                });
            }
            let usage_known = response.usage.input.is_some() || response.usage.output.is_some();
            context_estimate = response.usage.prompt_total() + response.usage.output.unwrap_or(0);

            let calls: Vec<(String, String, Value)> = response
                .message
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::ToolCall { call_id, name, input, .. } => Some((call_id.clone(), name.clone(), input.clone())),
                    _ => None,
                })
                .collect();
            self.messages.push(response.message.clone());
            match &response.stop {
                StopReason::Refusal => {
                    // 拒绝时也可能带着工具调用：补上「没跑」的结果，存下的对话下一句才接得上。
                    if !calls.is_empty() {
                        let skipped = calls
                            .iter()
                            .map(|(call_id, _, _)| Part::ToolResult {
                                call_id: call_id.clone(),
                                output: "模型拒绝了这个请求，这一步没跑。".into(),
                                is_error: true,
                            })
                            .collect();
                        self.push_user(skipped);
                    }
                    return End { reason: TurnEndReason::Refused, error: Some("模型拒绝了这个请求。".into()) };
                }
                StopReason::MaxTokens if calls.is_empty() => {
                    return End::limit("回答超过了最大输出长度，停在这里。接着说一句「继续」可以让它接着写。");
                }
                _ => {}
            }
            if calls.is_empty() {
                let steer = self.take_steer();
                if steer.is_empty() {
                    return End::done();
                }
                self.push_user(steer);
                continue;
            }
            if !usage_known {
                // 先把工具跑完（模型已经要了），下一次请求前再停。
                let results = self.run_tools(&calls, names).await;
                self.push_user(results);
                return End::limit("供应商这一次没报用量，花费算不了，先停下。接着说一句就能继续。");
            }
            let results = self.run_tools(&calls, names).await;
            let added: usize = results
                .iter()
                .map(|p| match p {
                    Part::ToolResult { output, .. } => output.len(),
                    _ => 0,
                })
                .sum();
            context_estimate += (added / 3) as u64;
            let mut parts = results;
            parts.extend(self.take_steer());
            self.push_user(parts);
            if self.ctl.interrupted {
                return End::interrupted();
            }
        }
    }

    /// 发一次请求：边收边发增量；站主打断就让 provider 停下。
    async fn ask_model(&mut self, request: ModelRequest, round: u32) -> Result<crate::gateway::ModelResponse, GatewayError> {
        let (tx, mut rx) = mpsc::unbounded_channel::<ModelEvent>();
        let mut fut = self.provider.complete(request, tx, self.cancel_rx.clone());
        let reasoning_id = format!("r{round}");
        let message_id = format!("m{round}");
        let mut open = true;
        let result = loop {
            tokio::select! {
                biased;
                r = &mut fut => break r,
                e = rx.recv(), if open => match e {
                    Some(e) => self.forward(e, &reasoning_id, &message_id),
                    None => open = false,
                },
                _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => {
                    let _ = self.cancel_tx.send(true);
                }
            }
        };
        while let Ok(e) = rx.try_recv() {
            self.forward(e, &reasoning_id, &message_id);
        }
        let response = result?;
        let mut reasoning = String::new();
        let mut opaque = None;
        let mut text = String::new();
        for part in &response.message.parts {
            match part {
                Part::Reasoning { text: t, opaque: o } => {
                    reasoning.push_str(t);
                    if opaque.is_none() {
                        opaque = o.clone();
                    }
                }
                Part::Text { text: t } => text.push_str(t),
                _ => {}
            }
        }
        if !reasoning.trim().is_empty() {
            self.emit(EventBody::ReasoningCompleted { item_id: reasoning_id, text: reasoning, opaque });
        }
        if !text.trim().is_empty() {
            self.emit(EventBody::MessageCompleted { item_id: message_id, text });
        }
        Ok(response)
    }

    fn forward(&self, e: ModelEvent, reasoning_id: &str, message_id: &str) {
        match e {
            ModelEvent::TextDelta { text } => self.emit(EventBody::MessageDelta { item_id: message_id.into(), text }),
            ModelEvent::ReasoningDelta { text } => self.emit(EventBody::ReasoningDelta { item_id: reasoning_id.into(), text }),
            ModelEvent::ToolCallStarted { .. } => {}
        }
    }

    /// 一批工具调用，按顺序跑；每个都有结果（被打断的写「没跑」），对话才接得上。
    async fn run_tools(&mut self, calls: &[(String, String, Value)], names: &[&'static str]) -> Vec<Part> {
        let mut results = Vec::new();
        for (call_id, name, input) in calls {
            if self.ctl.interrupted {
                results.push(Part::ToolResult { call_id: call_id.clone(), output: "已停止，这一步没跑。".into(), is_error: true });
                continue;
            }
            let started = Instant::now();
            self.emit(EventBody::ToolCallStarted {
                call_id: call_id.clone(),
                name: name.clone(),
                kind: tools::kind_of(name),
                title: tools::title_of(name, input),
                input: input.clone(),
            });
            let (status, output, exit_code, diff) = if !names.contains(&name.as_str()) {
                let why = if tools::ALL.contains(&name.as_str()) {
                    format!("这一档（{}）不能用 {name}。", level_label(self.level))
                } else {
                    format!("没有叫 {name} 的工具。")
                };
                (ToolStatus::Denied, why, None, vec![])
            } else {
                self.run_tool(call_id, name, input).await
            };
            let is_error = status != ToolStatus::Ok;
            self.emit(EventBody::ToolCallCompleted {
                call_id: call_id.clone(),
                status,
                output: Some(output.clone()),
                exit_code,
                diff: diff.clone(),
                duration_ms: Some(started.elapsed().as_millis() as u64),
            });
            if !diff.is_empty() {
                self.emit(EventBody::DiffUpdated { files: self.ctx.diff_summary() });
            }
            results.push(Part::ToolResult { call_id: call_id.clone(), output, is_error });
        }
        results
    }

    async fn run_tool(&mut self, call_id: &str, name: &str, input: &Value) -> (ToolStatus, String, Option<i32>, Vec<crate::gateway::event::FileDiff>) {
        match name {
            tools::UPDATE_PLAN => match parse_plan(input) {
                Ok((steps, explanation)) => {
                    let n = steps.len();
                    self.emit(EventBody::PlanUpdated { steps, explanation });
                    (ToolStatus::Ok, format!("任务清单更新了（{n} 步）。"), None, vec![])
                }
                Err(e) => (ToolStatus::Failed, e, None, vec![]),
            },
            tools::ASK_USER => self.ask_user(input).await,
            tools::BASH_RUN => self.bash(call_id, input).await,
            _ => {
                let ctx = self.ctx.clone();
                let (name, input) = (name.to_string(), input.clone());
                let r = tokio::task::spawn_blocking(move || tools::run_sync(&ctx, &name, &input))
                    .await
                    .unwrap_or_else(|_| tools::ToolResult::err("工具出错了。"));
                (if r.ok { ToolStatus::Ok } else { ToolStatus::Failed }, r.text, None, r.diff)
            }
        }
    }

    async fn ask_user(&mut self, input: &Value) -> (ToolStatus, String, Option<i32>, Vec<crate::gateway::event::FileDiff>) {
        let Some(question) = input.get("question").and_then(Value::as_str).filter(|q| !q.trim().is_empty()) else {
            return (ToolStatus::Failed, "缺 question。".into(), None, vec![]);
        };
        let options: Vec<String> = input
            .get("options")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|o| o.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        self.approvals += 1;
        let request_id = format!("ask-{}", self.approvals);
        self.emit(EventBody::UserInputRequested { request_id: request_id.clone(), question: question.to_string(), options, free_text: true });
        match self.ctl.wait_for(&request_id).await {
            Some(Control::Answer { answer, .. }) => {
                self.emit(EventBody::UserInputResolved { request_id, answer: answer.clone() });
                (ToolStatus::Ok, format!("站主回答：{answer}"), None, vec![])
            }
            _ => (ToolStatus::Cancelled, "站主没回答（任务停了）。".into(), None, vec![]),
        }
    }

    /// 弹一张审批卡片，等站主点。高危只有批准、不行。
    async fn approve(&mut self, call_id: &str, title: String, detail: String, high_risk: bool) -> ApprovalDecision {
        self.approvals += 1;
        let request_id = format!("approve-{}", self.approvals);
        let options = if high_risk {
            vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce]
        } else {
            vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce]
        };
        self.emit(EventBody::ApprovalRequested {
            request_id: request_id.clone(),
            call_id: Some(call_id.to_string()),
            kind: ToolKind::Run,
            title,
            detail: Some(detail),
            high_risk,
            options: options.clone(),
        });
        let decision = match self.ctl.wait_for(&request_id).await {
            Some(Control::Approve { decision, .. }) if options.contains(&decision) => decision,
            Some(Control::Approve { decision, .. }) if decision.allows() => ApprovalDecision::AllowOnce,
            _ => ApprovalDecision::DenyOnce,
        };
        if !self.ctl.interrupted {
            self.emit(EventBody::ApprovalResolved { request_id, decision, by: ResolvedBy::User });
        }
        decision
    }

    async fn bash(&mut self, call_id: &str, input: &Value) -> (ToolStatus, String, Option<i32>, Vec<crate::gateway::event::FileDiff>) {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if command.is_empty() {
            return (ToolStatus::Failed, "缺 command。".into(), None, vec![]);
        }
        match permission::check_command(self.level, &command, &self.ctx.root, &self.allowed) {
            Ask::No => {}
            Ask::HighRisk(risks) => {
                let kinds: Vec<&str> = risks.iter().map(|r| r.kind.label()).collect();
                let decision = self.approve(call_id, format!("高危：{}", kinds.join("、")), command.clone(), true).await;
                if !decision.allows() {
                    return (ToolStatus::Denied, format!("站主没批这条命令（{}）。换个办法，或者说明为什么非这样不可。", kinds.join("、")), None, vec![]);
                }
            }
            Ask::Confirm(kind) => {
                let decision = self.approve(call_id, format!("{}：要跑吗？", kind.label()), command.clone(), false).await;
                if decision == ApprovalDecision::AllowAlways {
                    self.allowed.insert(kind.rule().to_string());
                    if let Some(hook) = &self.hook {
                        hook(kind.rule());
                    }
                }
                if !decision.allows() {
                    return (ToolStatus::Denied, format!("站主没批（{}）。", kind.label()), None, vec![]);
                }
            }
        }
        if self.ctl.interrupted {
            return (ToolStatus::Cancelled, "已停止，命令没跑。".into(), None, vec![]);
        }
        let sandbox = permission::sandbox_for(self.level);
        let mut outcome = match self.exec(call_id, input, sandbox).await {
            Ok(o) => o,
            Err(e) => return (ToolStatus::Failed, e, None, vec![]),
        };
        if outcome.sandbox_denied() && self.level != PermissionLevel::Plan {
            let decision = self
                .approve(call_id, "高危：写到项目外（沙箱拦下了），出沙箱再跑一次？".into(), command.clone(), true)
                .await;
            if decision.allows() {
                match self.exec(call_id, input, bash::Sandbox::Off).await {
                    Ok(o) => outcome = o,
                    Err(e) => return (ToolStatus::Failed, e, None, vec![]),
                }
            }
        }
        let failed = !outcome.ok();
        let (shown, _) = spill::shape(&self.ctx, "bash", &outcome.output, failed);
        let mut text = outcome.status_line();
        if outcome.sandbox != bash::Sandbox::Off && outcome.sandbox_denied() {
            text.push_str("\n（沙箱没让它写到项目外，站主没批出沙箱。）");
        }
        if !shown.trim().is_empty() {
            text.push('\n');
            text.push_str(&shown);
        }
        let status = if outcome.cancelled {
            ToolStatus::Cancelled
        } else if failed {
            ToolStatus::Failed
        } else {
            ToolStatus::Ok
        };
        (status, text, outcome.exit_code, vec![])
    }

    async fn exec(&mut self, call_id: &str, input: &Value, sandbox: bash::Sandbox) -> Result<bash::BashOutcome, String> {
        let sink = self.sink.clone();
        let id = call_id.to_string();
        let ctx = self.ctx.clone();
        let fut = bash::run(&ctx, input, sandbox, self.cancel_rx.clone(), move |text| {
            sink.emit(EventBody::ToolCallOutputDelta { call_id: id.clone(), text });
        });
        tokio::pin!(fut);
        loop {
            tokio::select! {
                biased;
                r = &mut fut => return r,
                _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => {
                    let _ = self.cancel_tx.send(true);
                }
            }
        }
    }
}

fn level_label(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::Plan => "PLAN",
        PermissionLevel::Edit => "EDIT",
        PermissionLevel::Auto => "AUTO",
    }
}

fn parse_plan(input: &Value) -> Result<(Vec<PlanStep>, Option<String>), String> {
    let steps = input.get("steps").and_then(Value::as_array).ok_or("缺 steps。")?;
    let mut out = Vec::new();
    for s in steps {
        let text = s.get("text").and_then(Value::as_str).unwrap_or("").trim();
        if text.is_empty() {
            return Err("每一步都要有 text。".into());
        }
        let status = match s.get("status").and_then(Value::as_str).unwrap_or("pending") {
            "completed" | "done" => PlanStatus::Completed,
            "in_progress" | "doing" => PlanStatus::InProgress,
            _ => PlanStatus::Pending,
        };
        out.push(PlanStep { text: text.to_string(), status });
    }
    if out.iter().filter(|s| s.status == PlanStatus::InProgress).count() > 1 {
        return Err("同时只能有一步在做（in_progress）。".into());
    }
    let explanation = input.get("explanation").and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty());
    Ok((out, explanation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::{AgentEvent, Cost, TokenUsage};
    use crate::gateway::mock::{ScriptTurn, ScriptedModel};
    use crate::harness::tools::testing::TempProject;
    use serde_json::json;
    use std::time::Duration;

    fn usage(input: u64, output: u64) -> TokenUsage {
        TokenUsage { input: Some(input), output: Some(output), cache_read: Some(0), cache_write: Some(0), ..Default::default() }
    }

    fn turn(text: &str, calls: Vec<(&str, Value)>) -> ScriptTurn {
        ScriptTurn {
            reasoning: None,
            text: (!text.is_empty()).then(|| text.to_string()),
            tool_calls: calls.into_iter().map(|(n, v)| (n.to_string(), v)).collect(),
            usage: usage(1000, 100),
        }
    }

    fn config(p: &TempProject, model: &str) -> LoopConfig {
        LoopConfig {
            model_id: model.into(),
            max_rounds: DEFAULT_MAX_ROUNDS,
            budget: None,
            oneshot: false,
            allowed: HashSet::new(),
            live_spend: None,
            spill_dir: p.dir.with_extension("spill"),
            runtime_dir: p.dir.with_extension("runtime"),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            session_ref: Some("peck-s1".into()),
        }
    }

    fn task(p: &TempProject, level: PermissionLevel, prompt: &str) -> AgentTask {
        AgentTask {
            run_id: "run-t".into(),
            prompt: prompt.into(),
            cwd: p.dir.clone(),
            permission: level,
            model: None,
            effort: None,
            resume: None,
            peck_mcp: None,
        }
    }

    /// 跑一个回合；碰到审批、提问照 `answer` 回答。
    async fn drive(
        backend: &PeckBackend,
        task: AgentTask,
        mut answer: impl FnMut(&EventBody) -> Option<Control>,
    ) -> (RunOutcome, Vec<AgentEvent>) {
        let (sink, events) = EventSink::memory("peck");
        let (tx, rx) = mpsc::unbounded_channel();
        let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let sink2 = EventSink::new("run-t", None, "peck", None, move |e| {
            let _ = seen_tx.send(e.clone());
            sink.emit(e.body.clone());
        });
        let fut = backend.run(task, sink2, rx);
        tokio::pin!(fut);
        let outcome = loop {
            tokio::select! {
                r = &mut fut => break r.unwrap(),
                Some(e) = seen_rx.recv() => {
                    if let Some(c) = answer(&e.body) { let _ = tx.send(c); }
                }
            }
        };
        let events = events.lock().unwrap().clone();
        (outcome, events)
    }

    fn types(events: &[AgentEvent]) -> Vec<&'static str> {
        events.iter().map(|e| e.body.type_name()).collect()
    }

    #[tokio::test]
    async fn the_demo_script_reads_searches_edits_runs_plans_asks_and_finishes() {
        let p = TempProject::new();
        p.write("src/App.tsx", "export default () => <h1>Todo</h1>;\n")
            .write("package.json", "{\"scripts\":{\"test\":\"echo ok\"}}\n");
        let model = Arc::new(ScriptedModel::peck_demo(Duration::ZERO));
        let requests = model.requests.clone();
        let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]);
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Auto, "把标题改成中文"), |e| match e {
            EventBody::UserInputRequested { request_id, .. } => Some(Control::Answer { request_id: request_id.clone(), answer: "先不改".into() }),
            _ => None,
        })
        .await;
        assert_eq!(outcome.reason, TurnEndReason::Done, "{:?}", types(&events));
        assert_eq!(outcome.session_ref.as_deref(), Some("peck-s1"));
        assert_eq!(p.read("src/App.tsx"), "export default () => <h1>待办</h1>;\n");
        let t = types(&events);
        assert_eq!(t.first(), Some(&"session.started"));
        assert_eq!(t.last(), Some(&"turn.completed"));
        let started: Vec<String> = events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::ToolCallStarted { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(started, ["list_directory", "read_file", "search_text", "write_patch", "bash_run", "update_plan", "ask_user"]);
        assert!(t.contains(&"diff.updated") && t.contains(&"plan.updated") && t.contains(&"user_input.resolved"));
        assert!(t.contains(&"reasoning.delta") && t.contains(&"message.completed"));
        assert_eq!(t.iter().filter(|x| **x == "usage.updated").count(), 8);
        // 每次请求都把上一轮的工具结果带回去；ASK 的回答在最后一次请求里。
        let reqs = requests.lock().unwrap();
        assert_eq!(reqs.len(), 8);
        let last = reqs.last().unwrap();
        assert!(serde_json::to_string(&last.messages).unwrap().contains("先不改"));
        assert!(last.tools.iter().any(|t| t.name == "write_patch"));
        // 对话留着，下次接着
        assert_eq!(backend.conversation.lock().unwrap().len(), 16);
    }

    #[tokio::test]
    async fn plan_level_hides_edit_tools_and_refuses_them() {
        let p = TempProject::new();
        p.write("a.txt", "x\n");
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![("write_file", json!({"path":"a.txt","content":"y"}))]), turn("只能看看。", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]);
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Plan, "改一下"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert_eq!(p.read("a.txt"), "x\n");
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ToolCallCompleted { status: ToolStatus::Denied, output: Some(o), .. } if o.contains("PLAN"))));
        let names: Vec<String> = requests.lock().unwrap()[0].tools.iter().map(|t| t.name.clone()).collect();
        assert!(!names.contains(&"write_file".to_string()) && !names.contains(&"write_patch".to_string()));
        assert!(names.contains(&"bash_run".to_string()) && names.contains(&"ask_user".to_string()));
    }

    #[tokio::test]
    async fn high_risk_always_asks_and_a_no_keeps_the_file() {
        let p = TempProject::new();
        p.write("keep.txt", "x\n");
        for level in [PermissionLevel::Auto, PermissionLevel::Edit] {
            let model = Arc::new(ScriptedModel::new(
                "deepseek/deepseek-flash",
                vec![turn("", vec![("bash_run", json!({"command":"rm keep.txt"}))]), turn("好，不删。", vec![])],
                Duration::ZERO,
            ));
            let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]);
            let (outcome, events) = drive(&backend, task(&p, level, "清理"), |e| match e {
                EventBody::ApprovalRequested { request_id, high_risk, options, .. } => {
                    assert!(*high_risk);
                    assert!(!options.contains(&ApprovalDecision::AllowAlways), "高危不能总是允许");
                    Some(Control::Approve { request_id: request_id.clone(), decision: ApprovalDecision::DenyOnce })
                }
                _ => None,
            })
            .await;
            assert_eq!(outcome.reason, TurnEndReason::Done);
            assert!(p.dir.join("keep.txt").exists());
            assert!(events.iter().any(|e| matches!(e.body, EventBody::ApprovalResolved { decision: ApprovalDecision::DenyOnce, .. })));
            assert!(events.iter().any(|e| matches!(e.body, EventBody::ToolCallCompleted { status: ToolStatus::Denied, .. })));
        }
    }

    #[tokio::test]
    async fn edit_asks_before_installing_and_remembers_always_allow() {
        let p = TempProject::new();
        let script = || {
            vec![
                turn("", vec![("bash_run", json!({"command":"cargo add --help >/dev/null 2>&1 || true"}))]),
                turn("装好了。", vec![]),
            ]
        };
        let remembered = Arc::new(Mutex::new(Vec::<String>::new()));
        let r2 = remembered.clone();
        let backend = PeckBackend::new(Arc::new(ScriptedModel::new("deepseek/deepseek-flash", script(), Duration::ZERO)), config(&p, "deepseek/deepseek-flash"), vec![])
            .on_allow_always(move |rule| r2.lock().unwrap().push(rule.to_string()));
        let mut asked = 0;
        let (_, events) = drive(&backend, task(&p, PermissionLevel::Edit, "装依赖"), |e| match e {
            EventBody::ApprovalRequested { request_id, high_risk, options, .. } => {
                asked += 1;
                assert!(!*high_risk && options.contains(&ApprovalDecision::AllowAlways));
                Some(Control::Approve { request_id: request_id.clone(), decision: ApprovalDecision::AllowAlways })
            }
            _ => None,
        })
        .await;
        assert_eq!(asked, 1);
        assert!(events.iter().any(|e| matches!(e.body, EventBody::ToolCallCompleted { status: ToolStatus::Ok, .. })));
        assert_eq!(*remembered.lock().unwrap(), vec!["install-deps"]);
        // 记住以后不再问；AUTO 本来就不问
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.allowed.insert("install-deps".into());
        for (level, cfg) in [(PermissionLevel::Edit, cfg), (PermissionLevel::Auto, config(&p, "deepseek/deepseek-flash"))] {
            let backend = PeckBackend::new(Arc::new(ScriptedModel::new("deepseek/deepseek-flash", script(), Duration::ZERO)), cfg, vec![]);
            let (_, events) = drive(&backend, task(&p, level, "装依赖"), |e| {
                assert!(!matches!(e, EventBody::ApprovalRequested { .. }), "不该问");
                None
            })
            .await;
            assert!(!events.iter().any(|e| matches!(e.body, EventBody::ApprovalRequested { .. })));
        }
    }

    #[tokio::test]
    async fn round_and_cost_limits_stop_before_the_next_request() {
        let p = TempProject::new();
        let looping = || (0..10).map(|_| turn("", vec![("list_directory", json!({}))])).collect::<Vec<_>>();
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.max_rounds = 3;
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", looping(), Duration::ZERO));
        let requests = model.requests.clone();
        let backend = PeckBackend::new(model, cfg, vec![]);
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("3 轮")));

        // 花费上限：已经花了 $0.999，上限 $1，第一次请求以后就停
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.budget = Some(Budget { cap_usd: 1.0, spent_usd: 0.999_999 });
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", looping(), Duration::ZERO));
        let requests = model.requests.clone();
        let backend = PeckBackend::new(model, cfg, vec![]);
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 1, "第二次请求前停");
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("花费到了上限")));
        let priced = events.iter().find_map(|e| match &e.body {
            EventBody::UsageUpdated { cost: Cost { usd, .. }, .. } => *usd,
            _ => None,
        });
        assert!(priced.unwrap() > 0.0, "按价格表算了花费");

        // 已经到上限：一次都不发
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.budget = Some(Budget { cap_usd: 1.0, spent_usd: 1.0 });
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", looping(), Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, _) = drive(&PeckBackend::new(model, cfg, vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert!(requests.lock().unwrap().is_empty());

        // 同时跑的任务也在花：开工时才花了 $0.2，现算这个月已经 $1.5，一次都不发
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.budget = Some(Budget { cap_usd: 1.0, spent_usd: 0.2 });
        cfg.live_spend = Some(Arc::new(|| Some(1.5)));
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", looping(), Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, _) = drive(&PeckBackend::new(model, cfg, vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert!(requests.lock().unwrap().is_empty(), "别的任务花的也算");
    }

    #[tokio::test]
    async fn context_near_the_window_and_unknown_usage_stop() {
        let p = TempProject::new();
        let window = catalog::catalog().model("deepseek/deepseek-flash").and_then(|m| m.context_window).unwrap();
        let mut big = turn("", vec![("list_directory", json!({}))]);
        big.usage = usage(window * 9 / 10, 100);
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![big, turn("不会到这", vec![])], Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("上下文")));

        let mut unknown = turn("", vec![("list_directory", json!({}))]);
        unknown.usage = TokenUsage::default();
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![unknown, turn("不会到这", vec![])], Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, _) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn interrupting_a_command_stops_it_and_keeps_the_conversation_valid() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![("bash_run", json!({"command":"sleep 30"})), ("list_directory", json!({}))])],
            Duration::ZERO,
        ));
        let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]);
        let started = Instant::now();
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Auto, "x"), |e| match e {
            EventBody::ToolCallStarted { name, .. } if name == "bash_run" => Some(Control::Interrupt),
            _ => None,
        })
        .await;
        assert_eq!(outcome.reason, TurnEndReason::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(events.iter().any(|e| matches!(e.body, EventBody::ToolCallCompleted { status: ToolStatus::Cancelled, .. })));
        // 两个调用都有结果（第二个写「没跑」）
        let conv = backend.conversation.lock().unwrap();
        let results = conv.last().unwrap().parts.iter().filter(|p| matches!(p, Part::ToolResult { .. })).count();
        assert_eq!(results, 2);
    }

    #[tokio::test]
    async fn steering_text_goes_into_the_next_request_and_history_resumes() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![("list_directory", json!({}))]), turn("好的，用中文。", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let history = vec![
            Message { role: Role::User, parts: vec![Part::Text { text: "上一句".into() }] },
            Message { role: Role::Assistant, parts: vec![Part::Text { text: "上一个回答".into() }] },
        ];
        let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), history);
        let (outcome, _) = drive(&backend, task(&p, PermissionLevel::Edit, "这一句"), |e| match e {
            EventBody::ToolCallStarted { .. } => Some(Control::Append { text: "用中文回答".into() }),
            _ => None,
        })
        .await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let reqs = requests.lock().unwrap();
        assert_eq!(reqs[0].messages.len(), 3, "带着之前的对话");
        let second = serde_json::to_string(&reqs[1].messages).unwrap();
        assert!(second.contains("用中文回答"));
        // 一问一答交替
        for pair in reqs[1].messages.windows(2) {
            assert_ne!(pair[0].role, pair[1].role);
        }
    }

    #[tokio::test]
    async fn oneshot_only_reads() {
        let p = TempProject::new();
        p.write("a.txt", "hello\n");
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.oneshot = true;
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![("read_file", json!({"path":"a.txt"}))]), turn("里面是 hello。", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, cfg, vec![]), task(&p, PermissionLevel::Auto, "a.txt 里是什么"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let names: Vec<String> = requests.lock().unwrap()[0].tools.iter().map(|t| t.name.clone()).collect();
        assert_eq!(names, ["list_directory", "read_file", "search_text", "find_files", "bash_run"]);
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::MessageCompleted { text, .. } if text.contains("hello"))));
    }

    #[test]
    fn plans_allow_one_step_in_progress() {
        assert!(parse_plan(&json!({"steps":[{"text":"a","status":"in_progress"},{"text":"b","status":"in_progress"}]})).is_err());
        let (steps, _) = parse_plan(&json!({"steps":[{"text":"a","status":"completed"},{"text":"b","status":"in_progress"},{"text":"c"}]})).unwrap();
        assert_eq!(steps.iter().map(|s| s.status).collect::<Vec<_>>(), [PlanStatus::Completed, PlanStatus::InProgress, PlanStatus::Pending]);
    }
}
