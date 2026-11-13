//! Peck 循环：一个回合里反复「问模型 → 跑它要的工具 → 把结果放回去」，直到模型不再要工具。
//!
//! - 模型经 Gateway 的 [`ModelProvider`] 调（真 API 或假模型），对话用和供应商无关的 `Message` / `Part`；
//!   思考的私有数据原样挂在 `Part::Reasoning.opaque` 上，下一轮原样放回（换了供应商由 provider 丢掉）。
//! - 事件经 `EventSink` 发：和 CLI 引擎同一套（`session.started` 打头、`turn.completed` 收尾）。
//! - 上限：默认 40 轮；花费上限到了在下一次请求前停；供应商没报用量也停。失败、取消、重开都不自动重发。
//! - 上下文（v0.6）：系统提示分两层（Peck 的说明、项目说明 + 记忆，都打缓存断点）；每次发一句话带上环境块和
//!   引用的旧任务（`context.rs`、`history.rs`）；每次请求前估一遍大小（`context.updated`），到压缩线自动压缩
//!   三步（`compact.rs`），压完还超过 95% 才停。
//! - 验证（v0.6）：改了文件在后台跑 `.peck/verify.toml` 的验证命令；模型停下时没过就把失败交回去修，最多 3 次，
//!   同一个失败连续两次、两次之间没改文件都停；都过了不再叫模型（`monitor.rs`）。目标模式每次停下都跑目标那几条。
//! - 控制通道：审批、回答（ASK）、追加一句（下一次请求带上）、打断（停下当前请求或命令）。

use super::background;
use super::compact;
use super::context::{self, EnvInput, ProjectLayer};
use super::monitor::{self, Monitor};
use super::permission::{self, Ask};
use super::spill;
use super::tools::{self, bash, ToolContext};
use super::verify::Check;
use crate::gateway::caps::AgentCaps;
use crate::gateway::catalog::{self, Thinking};
use crate::gateway::event::{
    ApprovalDecision, BackgroundKind, CompactStep, EventBody, NoticeLevel, PermissionLevel, PlanStatus, PlanStep, ResolvedBy,
    ToolKind, ToolStatus, TurnEndReason, UsageScope, VerifyOutcome, VerifyTrigger,
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
/// 验证没过，最多交回去修几次（目标模式多一些）。
pub const FIX_ATTEMPTS: u32 = 3;
pub const GOAL_ATTEMPTS: u32 = 8;

/// 花费上限：这个月已经花了多少，上限多少（美元）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub cap_usd: f64,
    pub spent_usd: f64,
}

/// 这个月 Peck 循环和 ONE SHOT 一共花了多少（从事件日志现算，包括别的任务同时花的）。读不到为空。
pub type SpendProbe = std::sync::Arc<dyn Fn() -> Option<f64> + Send + Sync>;

/// `search_tasks`：（关键词，任务号）→ 给模型看的结果。
pub type HistoryProbe = Arc<dyn Fn(Option<String>, Option<u64>) -> Result<String, String> + Send + Sync>;

/// 上下文（v0.6 Phase 7）：项目层、环境块、引用、历史。
#[derive(Clone, Default)]
pub struct ContextConfig {
    /// 系统提示第二层：项目说明 + 记忆。
    pub project: ProjectLayer,
    /// 发这一句时带不带环境块（ONE SHOT、测试可以不带）。
    pub environment: bool,
    /// 这个任务之前几次改过的文件（环境块里写）。
    pub prior_changed: Vec<String>,
    /// `@` 进来的旧任务（`<referenced-task>`，放在这一句前面）。
    pub refs: Vec<String>,
    pub history: Option<HistoryProbe>,
    /// 压缩线（窗口的比例）；0 用默认的 80%。
    pub compact_at: f64,
}

/// 验证（v0.6 Phase 8）：`.peck/verify.toml` 的验证命令和目标模式。
#[derive(Clone, Default)]
pub struct VerifyPlan {
    pub checks: Vec<Check>,
    /// 目标模式：要过的那几条的名字（`*` 是全部）。空：不是目标模式。
    pub goal: Vec<String>,
}

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
    pub context: ContextConfig,
    pub verify: VerifyPlan,
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
                monitor: None,
                scale: 1.0,
                last_sent: 0,
                compacted_at_round: None,
                spill_counter: Default::default(),
                fixes: 0,
                last_failure: None,
                version_at_fix: None,
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
    /// 替模型盯验证命令（有验证命令、不是 PLAN 和 ONE SHOT 才有）。
    monitor: Option<Monitor>,
    /// 估的 tokens → 实报的 tokens 的校准系数（上一次请求算的）。
    scale: f64,
    /// 上一次请求发出去时估的大小（还没校准）。
    last_sent: u64,
    /// 上一次做交接摘要是第几轮（压过了又满就停，不来回压）。
    compacted_at_round: Option<u32>,
    spill_counter: std::sync::atomic::AtomicU32,
    /// 交回去修了几次。
    fixes: u32,
    /// 上一次没过的样子（认「同一个失败连续两次」）。
    last_failure: Option<u64>,
    /// 上一次交回去修时项目文件的版本（认「两次之间没改文件」）。
    version_at_fix: Option<u64>,
}

/// 一回合怎么结束的。
struct End {
    reason: TurnEndReason,
    error: Option<String>,
}

/// 模型停下以后看验证命令的结果。
enum Verdict {
    /// 都过了，或者没有要看的。
    Done,
    /// 没过：把这段交回模型修。
    Fix(String),
    Stop(End),
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
         - When the project has verify commands (listed in <peck-context>), Peck runs them for you after you edit and when you stop; if they fail you get the output in <peck-verify>. Fix the cause; don't claim success while a check fails.\n\
         - Use bash_background for dev servers, watchers and long test suites instead of bash_run, then read their output.\n\
         - search_tasks finds earlier tasks in this project; <referenced-task> blocks are earlier tasks the user pointed you to.\n\
         - When you learn something durable about this project (a convention, a command, a pitfall, a preference the user stated), propose it with memory; the user approves it at the end.\n\
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
        let mut first = Vec::new();
        if self.config.context.environment {
            let block = self.environment_block().await;
            first.push(Part::Text { text: block });
        }
        first.extend(self.config.context.refs.iter().map(|r| Part::Text { text: r.clone() }));
        first.push(Part::Text { text: self.task.prompt.clone() });
        self.push_user(first);
        let watch_checks = !self.config.oneshot && self.level != PermissionLevel::Plan && !self.config.verify.checks.is_empty();
        if watch_checks {
            let sandbox = permission::sandbox_for(self.level);
            self.monitor = Some(Monitor::new(self.config.verify.checks.clone(), self.ctx.clone(), self.sink.clone(), sandbox).await);
        }
        let end = self.rounds(&names).await;
        if let Some(m) = self.monitor.as_mut() {
            m.stop();
        }
        background::stop_owned(&self.task.run_id);
        // 收尾时再报一次大小：输入框旁的小环显示「下一句发出去大概多大」。
        self.report_context(&names, None);
        self.emit(EventBody::TurnCompleted { turn_id, reason: end.reason, error: end.error });
        RunOutcome { reason: end.reason, session_ref: self.config.session_ref.clone() }
    }

    /// 这一句话前面带上的环境：系统、git、任务清单、改过的文件、验证命令、后台命令。
    async fn environment_block(&self) -> String {
        let mut changed = self.config.context.prior_changed.clone();
        changed.sort();
        changed.dedup();
        let input = EnvInput {
            level: Some(self.level),
            plan: last_plan(&self.messages),
            changed,
            // PLAN 不替模型跑验证命令，就不提。
            checks: if self.level == PermissionLevel::Plan { vec![] } else { self.config.verify.checks.iter().map(|c| (c.name.clone(), c.run.clone())).collect() },
            background: background::list(&self.ctx.root).iter().filter(|p| p.running()).map(|p| background::describe_for_model(p)).collect(),
            goal: self.goal_names(),
        };
        let root = self.ctx.root.clone();
        tokio::task::spawn_blocking(move || context::environment_block(&root, &input)).await.unwrap_or_default()
    }

    fn goal_names(&self) -> Vec<String> {
        let goal = &self.config.verify.goal;
        if goal.iter().any(|g| g == "*") {
            return self.config.verify.checks.iter().map(|c| c.name.clone()).collect();
        }
        goal.clone()
    }

    fn system_blocks(&self) -> Vec<SystemBlock> {
        let mut blocks = vec![SystemBlock { text: system_prompt(&self.task, self.level, self.config.oneshot), cache: true }];
        if let Some(project) = self.config.context.project.text() {
            blocks.push(SystemBlock { text: project, cache: true });
        }
        blocks
    }

    /// 估现在的上下文多大（校准过），顺手发 `context.updated`。返回（估的总数，没校准的总数）。
    fn report_context(&mut self, names: &[&'static str], window: Option<u64>) -> (u64, u64) {
        let specs = tools::specs_for(names);
        let base = system_prompt(&self.task, self.level, self.config.oneshot);
        let project = self.config.context.project.text();
        let b = context::breakdown(&base, &specs, project.as_deref(), &self.messages);
        let raw = b.total();
        let used = context::scaled(raw, self.scale);
        let window = window.or_else(|| self.model_spec().and_then(|m| m.context_window));
        self.emit(EventBody::ContextUpdated {
            used,
            window,
            compact_at: window.map(|w| (w as f64 * self.compact_ratio()) as u64),
            parts: b.parts(self.scale),
        });
        (used, raw)
    }

    fn compact_ratio(&self) -> f64 {
        let r = self.config.context.compact_at;
        if r > 0.0 && r < 1.0 {
            r
        } else {
            compact::COMPACT_AT
        }
    }

    /// 到了压缩线：按顺序做三步，降到目标线以下就停。返回压完的大小；压不下去返回 `Err(停下的原因)`。
    async fn compact(&mut self, names: &[&'static str], window: u64, round: u32, request_base: &ModelRequest) -> Result<(), End> {
        let target = (window as f64 * compact::TARGET) as u64;
        let (mut used, _) = self.estimate(names);
        // 1. 落盘
        let before = used;
        let spill_dir = self.ctx.spill_dir.clone();
        let n = compact::spill_large(&mut self.messages, &spill_dir, &self.spill_counter);
        if n > 0 {
            used = self.estimate(names).0;
            self.emit(EventBody::ContextCompacted {
                step: CompactStep::Spill,
                before,
                after: used,
                text: format!("{n} 份长的工具输出落了盘，上下文里只留开头、结尾和路径"),
                summary: None,
            });
        }
        if used < target {
            return Ok(());
        }
        // 2. 清旧工具输出
        let before = used;
        let n = compact::clear_old(&mut self.messages);
        let after = self.estimate(names).0;
        if n > 0 || after < before {
            used = after;
            self.emit(EventBody::ContextCompacted {
                step: CompactStep::Clear,
                before,
                after: used,
                text: if n > 0 { format!("清掉了 {n} 份旧的工具输出（最近两轮的留着）") } else { "去掉了旧的思考和环境块".into() },
                summary: None,
            });
        }
        if used < target {
            return Ok(());
        }
        // 3. 交接摘要（同一轮里压过了还不够就别再压）
        if self.compacted_at_round.is_some_and(|r| round <= r + 1) {
            return Err(End::limit("上下文刚压缩过又满了，先停下。把这一步拆小一点，或者换一个新任务接着做。"));
        }
        let before = used;
        let Some(summary) = self.summarize(request_base).await? else {
            // 没写出完整的摘要：对话原样留着（不拿半截摘要换掉），下一轮又满就停。
            self.compacted_at_round = Some(round);
            self.emit(EventBody::Notice { level: NoticeLevel::Warn, text: "上下文快满了，模型没写出完整的交接摘要，对话原样留着。".into() });
            return Ok(());
        };
        let requests = compact::user_requests(&self.messages, 3);
        self.messages = compact::rebuild(&summary, &requests);
        self.compacted_at_round = Some(round);
        let after = self.estimate(names).0;
        self.emit(EventBody::ContextCompacted {
            step: CompactStep::Summary,
            before,
            after,
            text: "前面的对话换成了一份交接摘要（加上你最近说的几句）".into(),
            summary: Some(summary),
        });
        Ok(())
    }

    /// 估的大小（校准过、没校准）。不发事件。
    fn estimate(&self, names: &[&'static str]) -> (u64, u64) {
        let specs = tools::specs_for(names);
        let base = system_prompt(&self.task, self.level, self.config.oneshot);
        let project = self.config.context.project.text();
        let raw = context::breakdown(&base, &specs, project.as_deref(), &self.messages).total();
        (context::scaled(raw, self.scale), raw)
    }

    /// 让模型写交接摘要：同样的系统提示、工具和对话（缓存命中），最后一条后面接上要求；不显示成回答。
    /// 没写出来、写到一半被截断：`Ok(None)`。
    async fn summarize(&mut self, base: &ModelRequest) -> Result<Option<String>, End> {
        let mut messages = self.messages.clone();
        let ask = Part::Text { text: compact::SUMMARY_REQUEST.into() };
        match messages.last_mut() {
            Some(last) if last.role == Role::User => last.parts.push(ask),
            _ => messages.push(Message { role: Role::User, parts: vec![ask] }),
        }
        // 思考也算在输出里：给够，免得摘要还没写就用完。
        let request = ModelRequest { messages, max_output_tokens: base.max_output_tokens.min(16_000), ..base.clone() };
        let (tx, _rx) = mpsc::unbounded_channel::<ModelEvent>();
        let mut fut = self.provider.complete(request, tx, self.cancel_rx.clone());
        let result = loop {
            tokio::select! {
                biased;
                r = &mut fut => break r,
                _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => {
                    let _ = self.cancel_tx.send(true);
                }
            }
        };
        let response = match result {
            Ok(r) => r,
            Err(e) if e.kind == ErrorKind::Cancelled || self.ctl.interrupted => return Err(End::interrupted()),
            Err(e) => return Err(End::limit(format!("上下文快满了，压缩时写交接摘要失败：{}。换一个新任务接着做。", e.message))),
        };
        self.emit(EventBody::UsageUpdated { usage: response.usage.clone(), cost: response.cost.clone(), scope: UsageScope::Turn });
        if let Some(usd) = response.cost.usd {
            self.cost_usd += usd;
        }
        let text: String = response
            .message
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() || response.stop == StopReason::MaxTokens {
            return Ok(None);
        }
        Ok(Some(text.trim().to_string()))
    }

    async fn rounds(&mut self, names: &[&'static str]) -> End {
        let specs = tools::specs_for(names);
        let system = self.system_blocks();
        let spec = self.model_spec();
        let window = spec.and_then(|m| m.context_window);
        let max_output = spec.and_then(|m| m.max_output).unwrap_or(8192).clamp(1024, 32_000) as u32;
        let effort = self.task.effort.clone().filter(|e| spec.is_some_and(|m| m.efforts.contains(e)));
        let thinking = match spec.map(|m| m.thinking) {
            Some(Thinking::Always) => true,
            Some(Thinking::Switch) => effort.is_some(),
            _ => false,
        };
        let mut round = 0u32;
        loop {
            self.ctl.drain();
            if self.ctl.interrupted {
                return End::interrupted();
            }
            if round >= self.config.max_rounds {
                return End::limit(format!("到了 {} 轮的上限，先停下。文件和过程都留着，接着说一句就能继续。", self.config.max_rounds));
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
            // 上下文：估一遍；到了压缩线先压；压完还太满才停。
            let base_request = ModelRequest {
                model: self.config.model_id.clone(),
                system: system.clone(),
                messages: vec![],
                tools: specs.clone(),
                thinking,
                effort: effort.clone(),
                max_output_tokens: max_output,
            };
            let (mut used, _) = self.estimate(names);
            if let Some(w) = window {
                if used as f64 >= w as f64 * self.compact_ratio() {
                    let request_base = ModelRequest { messages: self.messages.clone(), ..base_request.clone() };
                    if let Err(end) = self.compact(names, w, round, &request_base).await {
                        return end;
                    }
                    used = self.estimate(names).0;
                    if used as f64 >= w as f64 * compact::STOP_AT {
                        self.report_context(names, Some(w));
                        return End::limit(format!(
                            "上下文用到了模型窗口的 {:.0}%，压缩以后还是太满，先停下。换一个新任务接着做，或者把要点再说一遍。",
                            used as f64 / w as f64 * 100.0
                        ));
                    }
                }
            }
            let (_, raw) = self.report_context(names, window);
            self.last_sent = raw;
            round += 1;
            let request = ModelRequest { messages: self.messages.clone(), ..base_request };
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
            // 校准：实报的提示大小 / 发出去时估的大小。
            let real = response.usage.prompt_total();
            if real > 0 && self.last_sent > 0 {
                self.scale = real as f64 / self.last_sent as f64;
            }

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
                if !steer.is_empty() {
                    self.push_user(steer);
                    continue;
                }
                // 模型停下了：替它看一遍验证命令。
                match self.verify_round().await {
                    Verdict::Done => return End::done(),
                    Verdict::Fix(text) => {
                        self.push_user(vec![Part::Text { text }]);
                        continue;
                    }
                    Verdict::Stop(end) => return end,
                }
            }
            if !usage_known {
                // 先把工具跑完（模型已经要了），下一次请求前再停。
                let results = self.run_tools(&calls, names).await;
                self.push_user(results);
                self.observe(&calls).await;
                return End::limit("供应商这一次没报用量，花费算不了，先停下。接着说一句就能继续。");
            }
            let results = self.run_tools(&calls, names).await;
            let mut parts = results;
            parts.extend(self.take_steer());
            self.push_user(parts);
            self.observe(&calls).await;
            if self.ctl.interrupted {
                return End::interrupted();
            }
        }
    }

    /// 一轮工具跑完：改了文件就让验证命令在后台跑（只在跑过会改东西的工具之后看）。
    async fn observe(&mut self, calls: &[(String, String, Value)]) {
        let mutating = calls.iter().any(|(_, n, _)| matches!(n.as_str(), tools::WRITE_FILE | tools::WRITE_PATCH | tools::BASH_RUN | tools::BASH_BACKGROUND));
        if !mutating {
            return;
        }
        let reported = self.ctx.diff_summary();
        if let Some(m) = self.monitor.as_mut() {
            m.observe(&reported).await;
        }
    }

    /// 模型停下以后的一轮验证：都过了（或没有要看的）收尾；没过交回去修；修够了、同一个失败连续两次、
    /// 两次之间没改文件都停。
    async fn verify_round(&mut self) -> Verdict {
        let goal_names = self.goal_names();
        let goal = !goal_names.is_empty();
        let Some(m) = self.monitor.as_mut() else { return Verdict::Done };
        let names = m.selection(&goal_names);
        if names.is_empty() {
            return Verdict::Done;
        }
        m.attempt = self.fixes;
        let trigger = if goal { VerifyTrigger::Goal } else { VerifyTrigger::End };
        let results = {
            let fut = m.settle(&names, trigger);
            tokio::pin!(fut);
            let mut interrupted = false;
            let r = loop {
                tokio::select! {
                    biased;
                    r = &mut fut => break Some(r),
                    _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => { interrupted = true; break None; }
                }
            };
            if interrupted {
                None
            } else {
                r
            }
        };
        let Some(results) = results else {
            if let Some(m) = self.monitor.as_mut() {
                m.stop();
            }
            return Verdict::Stop(End::interrupted());
        };
        let max = if goal { GOAL_ATTEMPTS } else { FIX_ATTEMPTS };
        let failed: Vec<monitor::Outcome> = results.iter().filter(|o| !o.passed).cloned().collect();
        let version = self.monitor.as_ref().map(|m| m.total_version).unwrap_or(0);
        let round = |outcome, reason: Option<String>| EventBody::VerifyRound { attempt: self.fixes, max_attempts: max, outcome, reason, goal };
        if failed.is_empty() {
            self.emit(round(VerifyOutcome::Passed, None));
            return Verdict::Done;
        }
        let sig = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            use std::hash::{Hash, Hasher};
            for o in &failed {
                o.signature.hash(&mut h);
            }
            h.finish()
        };
        let names_failed = failed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join("、");
        let stop = if self.fixes > 0 && self.version_at_fix == Some(version) {
            Some(format!("验证没过（{names_failed}），交回去修以后模型没改任何文件，先停下。"))
        } else if self.fixes > 0 && self.last_failure == Some(sig) {
            Some(format!("验证没过（{names_failed}）：同一个失败连续两次，先停下。"))
        } else if self.fixes >= max {
            Some(format!("验证没过（{names_failed}）：自动修了 {max} 次还没过，先停下。"))
        } else {
            None
        };
        if let Some(reason) = stop {
            self.emit(round(VerifyOutcome::Stopped, Some(reason.clone())));
            return Verdict::Stop(End::limit(format!("{reason}失败的输出在验证卡片里；接着说一句就能继续。")));
        }
        self.fixes += 1;
        self.last_failure = Some(sig);
        self.version_at_fix = Some(version);
        if let Some(m) = self.monitor.as_mut() {
            m.attempt = self.fixes;
        }
        self.emit(EventBody::VerifyRound { attempt: self.fixes, max_attempts: max, outcome: VerifyOutcome::Fixing, reason: None, goal });
        Verdict::Fix(monitor::failure_message(&failed, self.fixes, max, goal))
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
            tools::BASH_BACKGROUND => self.background(call_id, input).await,
            tools::MEMORY => self.memory(input),
            tools::SEARCH_TASKS => {
                let Some(probe) = self.config.context.history.clone() else {
                    return (ToolStatus::Failed, "这里读不到任务记录。".into(), None, vec![]);
                };
                let query = input.get("query").and_then(Value::as_str).map(str::to_string).filter(|q| !q.trim().is_empty());
                let number = tools::u64_arg(input, "number");
                match tokio::task::spawn_blocking(move || probe(query, number)).await {
                    Ok(Ok(text)) => (ToolStatus::Ok, text, None, vec![]),
                    Ok(Err(e)) => (ToolStatus::Failed, e, None, vec![]),
                    Err(_) => (ToolStatus::Failed, "读任务记录出错了。".into(), None, vec![]),
                }
            }
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

    /// `memory`：模型想记住的几条。只发事件，任务结束时站主在卡片上批。
    fn memory(&mut self, input: &Value) -> (ToolStatus, String, Option<i32>, Vec<crate::gateway::event::FileDiff>) {
        let notes: Vec<String> = input
            .get("notes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).filter_map(super::history::clean_note).collect())
            .unwrap_or_default();
        if notes.is_empty() {
            return (ToolStatus::Failed, "notes 是空的。".into(), None, vec![]);
        }
        let notes: Vec<String> = notes.into_iter().take(super::history::NOTES_PER_CALL).collect();
        let n = notes.len();
        self.emit(EventBody::MemoryProposed { notes });
        (ToolStatus::Ok, format!("记下了 {n} 条，任务结束时站主决定要不要记住。"), None, vec![])
    }

    /// `bash_background`：起、读、停、列后台命令。起的时候照 `bash_run` 的规矩审批（EDIT 当「起常驻进程」问一次）。
    async fn background(&mut self, call_id: &str, input: &Value) -> (ToolStatus, String, Option<i32>, Vec<crate::gateway::event::FileDiff>) {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("list");
        let run_id = self.task.run_id.clone();
        let visible = |p: &background::BgProc| p.owner.as_deref() == Some(run_id.as_str()) || p.kind == BackgroundKind::Server;
        let fail = |t: String| (ToolStatus::Failed, t, None, vec![]);
        match action {
            "start" => {
                let command = input.get("command").and_then(Value::as_str).unwrap_or("").trim().to_string();
                if command.is_empty() {
                    return fail("缺 command。".into());
                }
                let mine = background::list(&self.ctx.root).iter().filter(|p| p.owner.as_deref() == Some(run_id.as_str()) && p.running()).count();
                if mine >= background::PER_RUN {
                    return fail(format!("这个任务已经有 {mine} 个后台命令在跑了，先停掉一个。"));
                }
                match permission::check_command(self.level, &command, &self.ctx.root, &self.allowed) {
                    Ask::HighRisk(risks) => {
                        let kinds: Vec<&str> = risks.iter().map(|r| r.kind.label()).collect();
                        let decision = self.approve(call_id, format!("高危：{}", kinds.join("、")), command.clone(), true).await;
                        if !decision.allows() {
                            return (ToolStatus::Denied, format!("站主没批这条命令（{}）。", kinds.join("、")), None, vec![]);
                        }
                    }
                    ask => {
                        // EDIT：这条命令自己要问的（比如装依赖）照问；起在后台本身再问一次「起常驻进程」。
                        let mut kinds: Vec<permission::Confirm> = Vec::new();
                        if let Ask::Confirm(c) = ask {
                            kinds.push(c);
                        }
                        let long = permission::Confirm::LongRunning;
                        if self.level == PermissionLevel::Edit && !self.allowed.contains(long.rule()) && !kinds.contains(&long) {
                            kinds.push(long);
                        }
                        for kind in kinds {
                            let decision = self.approve(call_id, format!("{}：要在后台跑吗？", kind.label()), command.clone(), false).await;
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
                }
                if self.ctl.interrupted {
                    return (ToolStatus::Cancelled, "已停止，命令没起。".into(), None, vec![]);
                }
                let name = input
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| command.split_whitespace().take(2).collect::<Vec<_>>().join(" "));
                let sink = self.sink.clone();
                let spec = background::Spec {
                    workspace: self.ctx.root.clone(),
                    owner: Some(run_id.clone()),
                    name,
                    command,
                    cwd: input.get("cwd").and_then(Value::as_str).map(str::to_string),
                    kind: BackgroundKind::Command,
                    ready: None,
                    sandbox: permission::sandbox_for(self.level),
                    env: self.ctx.env.clone(),
                    runtime_dir: self.ctx.runtime_dir.clone(),
                    log_dir: self.ctx.spill_dir.clone(),
                };
                let proc_ = match background::start(spec, Some(Arc::new(move |i: &background::BgInfo| {
                    sink.emit(background::event_of(i));
                }))) {
                    Ok(p) => p,
                    Err(e) => return fail(e),
                };
                // 等一小会儿，把开头的输出一起给模型。
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(1500)) => {}
                    _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => {}
                }
                let first = proc_.read_new();
                (ToolStatus::Ok, format!("{}
完整输出写在 {}（read_file 读得到）。
{first}", proc_.status_line(), proc_.log.display()), None, vec![])
            }
            "read" | "stop" => {
                let id = input.get("id").and_then(Value::as_str).unwrap_or("").trim();
                let Some(proc_) = background::get(id).filter(|p| p.workspace == self.ctx.root && visible(p)) else {
                    return fail(format!("没有 id 是 {id} 的后台命令（list 看有哪些）。"));
                };
                if action == "stop" {
                    if proc_.kind == BackgroundKind::Server {
                        return fail("常驻的 dev server 由站主起停，你只能读它的输出。".into());
                    }
                    proc_.stop();
                    for _ in 0..40 {
                        if !proc_.running() {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    return (ToolStatus::Ok, proc_.status_line(), None, vec![]);
                }
                let wait = tools::u64_arg(input, "wait_sec").unwrap_or(0).min(60);
                tokio::select! {
                    _ = proc_.wait_change(std::time::Duration::from_secs(wait)) => {}
                    _ = self.ctl.until_interrupt(), if !self.ctl.interrupted => {}
                }
                let fresh = proc_.read_new();
                let body = if fresh.is_empty() { "（上次读过以后没有新输出）".to_string() } else { fresh };
                (ToolStatus::Ok, format!("{}
{body}", proc_.status_line()), proc_.info().exit_code, vec![])
            }
            "list" => {
                let all: Vec<String> = background::list(&self.ctx.root).iter().filter(|p| visible(p)).map(|p| p.status_line()).collect();
                (ToolStatus::Ok, if all.is_empty() { "没有后台命令。".into() } else { all.join("
") }, None, vec![])
            }
            other => fail(format!("action 只有 start、read、stop、list（收到 {other}）。")),
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

/// 对话里最近一份任务清单（`update_plan` 的参数）。
fn last_plan(messages: &[Message]) -> Vec<PlanStep> {
    messages
        .iter()
        .rev()
        .flat_map(|m| m.parts.iter().rev())
        .find_map(|p| match p {
            Part::ToolCall { name, input, .. } if name == tools::UPDATE_PLAN => parse_plan(input).ok().map(|(s, _)| s),
            _ => None,
        })
        .unwrap_or_default()
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
            context: ContextConfig::default(),
            verify: VerifyPlan::default(),
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

    fn window() -> u64 {
        catalog::catalog().model("deepseek/deepseek-flash").and_then(|m| m.context_window).unwrap()
    }

    fn compactions(events: &[AgentEvent]) -> Vec<CompactStep> {
        events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::ContextCompacted { step, .. } => Some(*step),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_long_task_compacts_and_keeps_going() {
        let p = TempProject::new();
        // 一个读出来很长的文件：第一次读完，上下文按实报已经到了窗口的 79%。
        let big: String = (0..1500).map(|i| format!("line {i:04} {}\n", "x".repeat(20))).collect();
        p.write("big.txt", &big);
        let mut first = turn("", vec![("read_file", json!({"path":"big.txt"}))]);
        first.usage = usage(window() * 30 / 100, 100);
        let summary = turn("## 目标\n读 big.txt 然后收尾。\n## 状态\n读完了。", vec![]);
        let mut after = turn("", vec![("list_directory", json!({}))]);
        after.usage = usage(2000, 100);
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![first, summary, after, turn("接着做完了。", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let backend = PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]);
        let (outcome, events) = drive(&backend, task(&p, PermissionLevel::Edit, "读 big.txt"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done, "{:?}", types(&events));
        let steps = compactions(&events);
        assert!(steps.contains(&CompactStep::Summary), "{steps:?}");
        let summary_text = events.iter().find_map(|e| match &e.body {
            EventBody::ContextCompacted { summary: Some(s), .. } => Some(s.clone()),
            _ => None,
        });
        assert!(summary_text.unwrap().contains("读完了"));
        let reqs = requests.lock().unwrap();
        // 摘要那次请求带着整段对话和「写交接摘要」；之后的对话只剩一条交接
        let ask = reqs.iter().position(|r| serde_json::to_string(&r.messages).unwrap().contains("<peck-compact>")).unwrap();
        let next = &reqs[ask + 1];
        assert_eq!(next.messages.len(), 1);
        assert!(serde_json::to_string(&next.messages).unwrap().contains("<peck-handoff>"));
        assert!(serde_json::to_string(&next.messages).unwrap().contains("读 big.txt"), "站主说的话留着");
        assert!(!events.iter().any(|e| matches!(&e.body, EventBody::MessageCompleted { text, .. } if text.contains("读完了"))), "摘要不当成回答");
        // 大小一直在报，最后一条比压缩前小
        let used: Vec<u64> = events.iter().filter_map(|e| match &e.body { EventBody::ContextUpdated { used, .. } => Some(*used), _ => None }).collect();
        assert!(used.len() >= 4);
        assert!(used.last() < used.iter().max());
        let conv = backend.conversation.lock().unwrap();
        assert!(matches!(&conv[0].parts[0], Part::Text { text } if text.starts_with("<peck-handoff>")));
    }

    #[tokio::test]
    async fn still_too_full_after_compacting_stops_and_unknown_usage_stops() {
        let p = TempProject::new();
        let mut big = turn("", vec![("list_directory", json!({}))]);
        big.usage = usage(window() * 99 / 100, 100);
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![big, turn("摘要", vec![]), turn("不会到这", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 2, "一次干活、一次写摘要");
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("压缩以后还是太满")));

        let mut unknown = turn("", vec![("list_directory", json!({}))]);
        unknown.usage = TokenUsage::default();
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![unknown, turn("不会到这", vec![])], Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, _) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn project_layer_environment_refs_and_history_reach_the_model() {
        let p = TempProject::new();
        p.write("AGENTS.md", "提交说明用英文。\n");
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![
                turn("", vec![("search_tasks", json!({"query":"登录"})), ("memory", json!({"notes":["- 提交说明用英文小写", "  "]}))]),
                turn("好。", vec![]),
            ],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let mut cfg = config(&p, "deepseek/deepseek-flash");
        cfg.context = ContextConfig {
            project: ProjectLayer::load(&p.dir, vec!["测试用 pnpm test".into()]),
            environment: true,
            prior_changed: vec!["src/a.ts".into()],
            refs: vec!["<referenced-task number=\"12\">\n修过登录\n</referenced-task>".into()],
            history: Some(Arc::new(|q: Option<String>, _n: Option<u64>| Ok(format!("找到：任务 12 {}", q.unwrap_or_default())))),
            compact_at: 0.0,
        };
        cfg.verify.checks = vec![Check { name: "check".into(), run: "true".into(), when: vec![], timeout: None }];
        let (outcome, events) = drive(&PeckBackend::new(model, cfg, vec![]), task(&p, PermissionLevel::Edit, "接着修"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        let reqs = requests.lock().unwrap();
        let first = &reqs[0];
        assert_eq!(first.system.len(), 2, "Peck 的说明 + 项目层");
        assert!(first.system.iter().all(|b| b.cache));
        assert!(first.system[1].text.contains("提交说明用英文") && first.system[1].text.contains("测试用 pnpm test"));
        let parts = &first.messages[0].parts;
        assert!(matches!(&parts[0], Part::Text { text } if text.starts_with("<peck-context>") && text.contains("src/a.ts") && text.contains("check: `true`")));
        assert!(matches!(&parts[1], Part::Text { text } if text.starts_with("<referenced-task")));
        assert!(matches!(&parts[2], Part::Text { text } if text == "接着修"));
        let second = serde_json::to_string(&reqs[1].messages).unwrap();
        assert!(second.contains("找到：任务 12 登录"));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::MemoryProposed { notes } if notes == &vec!["提交说明用英文小写".to_string()])));
        let parts = events.iter().find_map(|e| match &e.body { EventBody::ContextUpdated { parts, .. } => Some(parts.clone()), _ => None }).unwrap();
        assert!(parts.iter().any(|x| x.key == "project" && x.tokens > 0));
    }

    #[tokio::test]
    async fn background_commands_do_not_block_and_stop_with_the_task() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![
                turn("", vec![("bash_background", json!({"action":"start","command":"echo up; sleep 30","name":"dev"}))]),
                turn("", vec![("bash_background", json!({"action":"list"}))]),
                turn("起好了。", vec![]),
            ],
            Duration::ZERO,
        ));
        let started = Instant::now();
        // 自己的任务号：别的测试收尾时停的是它们自己的后台命令。
        let mut t = task(&p, PermissionLevel::Auto, "起 dev");
        t.run_id = format!("run-bg-{}", uuid::Uuid::new_v4().simple());
        let (outcome, events) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), t, |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(started.elapsed() < Duration::from_secs(15), "不等它跑完");
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ToolCallCompleted { output: Some(o), .. } if o.contains("up"))), "开头的输出给了模型");
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ToolCallCompleted { output: Some(o), .. } if o.contains("在跑"))), "{:#?}", events.iter().filter(|e| matches!(e.body, EventBody::ToolCallCompleted { .. })).collect::<Vec<_>>());
        for _ in 0..60 {
            if background::list(&p.dir).iter().all(|x| !x.running()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(background::list(&p.dir).iter().all(|x| !x.running()), "任务结束就停");
        let statuses: Vec<crate::gateway::event::BackgroundStatus> = events.iter().filter_map(|e| match &e.body { EventBody::BackgroundUpdated { status, .. } => Some(*status), _ => None }).collect();
        assert_eq!(statuses.first(), Some(&crate::gateway::event::BackgroundStatus::Running));
    }

    #[tokio::test]
    async fn edit_asks_before_a_background_command() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![("bash_background", json!({"action":"start","command":"sleep 5"}))]), turn("不起了。", vec![])],
            Duration::ZERO,
        ));
        let mut asked = 0;
        let (_, events) = drive(&PeckBackend::new(model, config(&p, "deepseek/deepseek-flash"), vec![]), task(&p, PermissionLevel::Edit, "x"), |e| match e {
            EventBody::ApprovalRequested { request_id, title, .. } => {
                asked += 1;
                assert!(title.contains("起常驻进程"));
                Some(Control::Approve { request_id: request_id.clone(), decision: ApprovalDecision::DenyOnce })
            }
            _ => None,
        })
        .await;
        assert_eq!(asked, 1);
        assert!(events.iter().any(|e| matches!(e.body, EventBody::ToolCallCompleted { status: ToolStatus::Denied, .. })));
        assert!(background::list(&p.dir).is_empty());
    }

    fn verify_cfg(p: &TempProject, run: &str) -> LoopConfig {
        let mut cfg = config(p, "deepseek/deepseek-flash");
        cfg.verify.checks = vec![Check { name: "check".into(), run: run.into(), when: vec!["**/*.txt".into()], timeout: Some(30) }];
        cfg
    }

    fn rounds_of(events: &[AgentEvent]) -> Vec<(VerifyOutcome, u32)> {
        events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::VerifyRound { outcome, attempt, .. } => Some((*outcome, *attempt)),
                _ => None,
            })
            .collect()
    }

    fn write(path: &str) -> (&'static str, Value) {
        ("write_file", json!({"path": path, "content": "x\n"}))
    }

    #[tokio::test]
    async fn a_failing_check_goes_back_and_the_fix_passes_without_another_call() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![write("bad.txt")]), turn("改好了。", vec![]), turn("", vec![write("good.txt")]), turn("这次真的好了。", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, verify_cfg(&p, "test -f good.txt || { echo 'error: good.txt missing'; exit 1; }"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done, "{:?}", types(&events));
        assert_eq!(rounds_of(&events), vec![(VerifyOutcome::Fixing, 1), (VerifyOutcome::Passed, 1)]);
        let reqs = requests.lock().unwrap();
        assert_eq!(reqs.len(), 4, "过了就不再叫模型");
        let third = serde_json::to_string(&reqs[2].messages).unwrap();
        assert!(third.contains("<peck-verify>") && third.contains("good.txt missing") && third.contains("1/3"));
        let starts: Vec<VerifyTrigger> = events.iter().filter_map(|e| match &e.body { EventBody::VerifyStarted { trigger, .. } => Some(*trigger), _ => None }).collect();
        assert!(starts.contains(&VerifyTrigger::Edit), "改了文件后台先跑：{starts:?}");
    }

    #[tokio::test]
    async fn the_same_failure_twice_stops() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![write("a.txt")]), turn("好了。", vec![]), turn("", vec![write("b.txt")]), turn("又好了。", vec![]), turn("不会到这", vec![])],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, verify_cfg(&p, "echo 'error[E1]: nope (0.01s)'; exit 1"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 4);
        assert_eq!(rounds_of(&events), vec![(VerifyOutcome::Fixing, 1), (VerifyOutcome::Stopped, 1)]);
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("同一个失败连续两次")));
    }

    #[tokio::test]
    async fn no_edit_between_attempts_stops_and_three_fixes_is_the_limit() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("", vec![write("a.txt")]), turn("好了。", vec![]), turn("我觉得没问题。", vec![]), turn("不会到这", vec![])],
            Duration::ZERO,
        ));
        let (outcome, events) = drive(&PeckBackend::new(model, verify_cfg(&p, "exit 1"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert!(matches!(&events.last().unwrap().body, EventBody::TurnCompleted { error: Some(e), .. } if e.contains("没改任何文件")));

        // 每次都换个失败、每次都改了文件：修 3 次还没过就停
        let p = TempProject::new();
        let mut script = vec![];
        for i in 0..5 {
            script.push(turn("", vec![write(&format!("f{i}.txt"))]));
            script.push(turn("好了。", vec![]));
        }
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", script, Duration::ZERO));
        let requests = model.requests.clone();
        let (outcome, events) = drive(&PeckBackend::new(model, verify_cfg(&p, "echo \"error: $(ls *.txt | wc -l) files\"; exit 1"), vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Limit);
        assert_eq!(requests.lock().unwrap().len(), 8);
        let r = rounds_of(&events);
        assert_eq!(r.iter().filter(|x| x.0 == VerifyOutcome::Fixing).count(), 3);
        assert_eq!(r.last(), Some(&(VerifyOutcome::Stopped, 3)));
    }

    #[tokio::test]
    async fn goal_mode_runs_the_goal_even_without_edits_and_plan_never_verifies() {
        let p = TempProject::new();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![turn("先看看。", vec![]), turn("", vec![("write_file", json!({"path":"done.txt","content":"ok"}))]), turn("做到了。", vec![])],
            Duration::ZERO,
        ));
        let mut cfg = verify_cfg(&p, "test -f done.txt");
        cfg.verify.goal = vec!["check".into()];
        let (outcome, events) = drive(&PeckBackend::new(model, cfg, vec![]), task(&p, PermissionLevel::Edit, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done, "{:?}", types(&events));
        assert_eq!(rounds_of(&events), vec![(VerifyOutcome::Fixing, 1), (VerifyOutcome::Passed, 1)]);
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::VerifyRound { goal: true, max_attempts: GOAL_ATTEMPTS, .. })));

        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![turn("只看看。", vec![])], Duration::ZERO));
        let (outcome, events) = drive(&PeckBackend::new(model, verify_cfg(&p, "exit 1"), vec![]), task(&p, PermissionLevel::Plan, "x"), |_| None).await;
        assert_eq!(outcome.reason, TurnEndReason::Done);
        assert!(rounds_of(&events).is_empty(), "PLAN 不改文件，不看验证");
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
        assert_eq!(names, ["list_directory", "read_file", "search_text", "find_files", "bash_run", "search_tasks"]);
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::MessageCompleted { text, .. } if text.contains("hello"))));
    }

    #[test]
    fn plans_allow_one_step_in_progress() {
        assert!(parse_plan(&json!({"steps":[{"text":"a","status":"in_progress"},{"text":"b","status":"in_progress"}]})).is_err());
        let (steps, _) = parse_plan(&json!({"steps":[{"text":"a","status":"completed"},{"text":"b","status":"in_progress"},{"text":"c"}]})).unwrap();
        assert_eq!(steps.iter().map(|s| s.status).collect::<Vec<_>>(), [PlanStatus::Completed, PlanStatus::InProgress, PlanStatus::Pending]);
    }
}
