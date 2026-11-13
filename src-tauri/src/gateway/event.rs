//! 统一事件：Peck 自己的循环和每个 CLI 引擎都转成这一套，界面只认它。
//!
//! 和 `src/lib/agentEvents.ts` 一一对应（改一边就改另一边；`tests::ts_mirror_lists_every_type` 会查）。
//! 每条事件在事件日志里另存一份原件（`raw` 列），不认识的东西变成 `raw` 事件，不丢、不报错。
//!
//! 用量的口径（各家不一样，转换时统一成这样）：
//! - `input`：没走缓存的输入 tokens；缓存读、缓存写各算各的，三者相加才是整个提示。
//! - `output`：全部输出，**包括**思考；`reasoning` 只是其中思考的那部分，给界面看。

use super::caps::AgentCaps;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一条事件。`seq` 在一个任务里从 0 递增，由 `EventSink` 编号；`at` 是毫秒时间戳。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub seq: u64,
    pub at: i64,
    /// 子任务里发生的事件带子任务的 id（见 `subagent.started`）；主线为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,
    #[serde(flatten)]
    pub body: EventBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum EventBody {
    // ── 会话 ──
    /// 引擎起来了：它是谁、能做什么、带了哪些工具和 MCP。
    #[serde(rename = "session.started")]
    SessionStarted {
        /// 引擎 id：`peck`（Peck 自己的循环）、`codex`、`claude-code`、`muse`、`grok`、`antigravity`。
        engine: String,
        #[serde(default)]
        model: Option<String>,
        caps: AgentCaps,
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        mcp: Vec<McpServer>,
        /// 续接用的会话号（Codex thread、Claude Code session、ACP session）。
        #[serde(default)]
        session_ref: Option<String>,
        /// CLI 的版本（检测时记下的），API 为空。
        #[serde(default)]
        version: Option<String>,
    },
    #[serde(rename = "session.config_changed")]
    SessionConfigChanged {
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        permission: Option<PermissionLevel>,
        #[serde(default)]
        effort: Option<String>,
    },

    // ── 回合 ──
    #[serde(rename = "turn.started")]
    TurnStarted { turn_id: String },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        turn_id: String,
        reason: TurnEndReason,
        #[serde(default)]
        error: Option<String>,
    },

    // ── 内容 ──
    /// 回答的一段新字。同一个 `item_id` 的 delta 连起来就是 `message.completed` 的 `text`。
    #[serde(rename = "message.delta")]
    MessageDelta { item_id: String, text: String },
    #[serde(rename = "message.completed")]
    MessageCompleted { item_id: String, text: String },
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { item_id: String, text: String },
    /// 思考结束。`opaque` 是供应商的私有数据（签名、密文），原样挂着，只在同一来源时回放。
    #[serde(rename = "reasoning.completed")]
    ReasoningCompleted {
        item_id: String,
        text: String,
        #[serde(default)]
        opaque: Option<OpaqueReasoning>,
    },

    // ── 工具 ──
    #[serde(rename = "tool_call.started")]
    ToolCallStarted {
        call_id: String,
        /// 工具原名（`read_file`、`Bash`、`commandExecution`……）。
        name: String,
        kind: ToolKind,
        /// 给人看的一行：「读 src/App.tsx」「跑 npm test」。
        title: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(rename = "tool_call.output_delta")]
    ToolCallOutputDelta { call_id: String, text: String },
    #[serde(rename = "tool_call.completed")]
    ToolCallCompleted {
        call_id: String,
        status: ToolStatus,
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        exit_code: Option<i32>,
        /// 改文件的工具带上改了什么。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        diff: Vec<FileDiff>,
        #[serde(default)]
        duration_ms: Option<u64>,
    },

    // ── 改动、计划 ──
    /// 这一回合到现在为止改了哪些文件（整份汇总，后一条替掉前一条）。
    #[serde(rename = "diff.updated")]
    DiffUpdated { files: Vec<FileDiff> },
    /// 任务清单（整份，后一条替掉前一条）。
    #[serde(rename = "plan.updated")]
    PlanUpdated {
        steps: Vec<PlanStep>,
        #[serde(default)]
        explanation: Option<String>,
    },

    // ── 交互 ──
    /// 要站主批的事。回答走控制通道 `Control::Approve`。
    #[serde(rename = "approval.requested")]
    ApprovalRequested {
        request_id: String,
        #[serde(default)]
        call_id: Option<String>,
        kind: ToolKind,
        title: String,
        /// 命令原文、路径之类。
        #[serde(default)]
        detail: Option<String>,
        /// 高危（删文件、推送、写到项目外、改系统设置）：不管哪档都弹卡片。
        #[serde(default)]
        high_risk: bool,
        options: Vec<ApprovalDecision>,
    },
    #[serde(rename = "approval.resolved")]
    ApprovalResolved {
        request_id: String,
        decision: ApprovalDecision,
        by: ResolvedBy,
    },
    /// 模型要问站主的事（ASK 卡片）。回答走 `Control::Answer`。
    #[serde(rename = "user_input.requested")]
    UserInputRequested {
        request_id: String,
        question: String,
        #[serde(default)]
        options: Vec<String>,
        /// 能不能自己写答案。
        #[serde(default = "yes")]
        free_text: bool,
    },
    #[serde(rename = "user_input.resolved")]
    UserInputResolved { request_id: String, answer: String },

    // ── 子任务 ──
    #[serde(rename = "subagent.started")]
    SubagentStarted {
        subagent_id: String,
        #[serde(default)]
        parent_call_id: Option<String>,
        title: String,
    },
    #[serde(rename = "subagent.completed")]
    SubagentCompleted {
        subagent_id: String,
        status: ToolStatus,
    },

    // ── 用量、额度 ──
    #[serde(rename = "usage.updated")]
    UsageUpdated {
        usage: TokenUsage,
        cost: Cost,
        scope: UsageScope,
    },
    /// 订阅窗口的余量（Codex、Muse、Grok）。和美元分开，不相加。
    #[serde(rename = "quota.updated")]
    QuotaUpdated {
        /// 引擎 id。
        provider: String,
        windows: Vec<QuotaWindow>,
    },

    // ── 上下文（v0.6）──
    /// 这份上下文现在多大、分成哪几块（输入框旁的用量小环）。有实报的用量就按实报校准。
    #[serde(rename = "context.updated")]
    ContextUpdated {
        /// 估的 tokens（下一次请求的整个提示）。
        used: u64,
        #[serde(default)]
        window: Option<u64>,
        /// 到这个数自动压缩（压缩线）。
        #[serde(default)]
        compact_at: Option<u64>,
        #[serde(default)]
        parts: Vec<ContextPart>,
    },
    /// 自动压缩的一步：落盘、清旧工具输出、交接摘要。
    #[serde(rename = "context.compacted")]
    ContextCompacted {
        step: CompactStep,
        before: u64,
        after: u64,
        /// 给人看的一句：「落盘了 3 份长输出」。
        text: String,
        /// 交接摘要的原文（`summary` 这一步才有）。
        #[serde(default)]
        summary: Option<String>,
    },
    /// 模型想记住的几条（任务结束时站主批）。
    #[serde(rename = "memory.proposed")]
    MemoryProposed { notes: Vec<String> },
    /// 站主批完了：记住了哪几条、没要哪几条（任务结束后追加进事件日志）。
    #[serde(rename = "memory.reviewed")]
    MemoryReviewed {
        saved: Vec<String>,
        #[serde(default)]
        skipped: Vec<String>,
    },

    // ── 验证和后台（v0.6）──
    /// Peck 替模型跑一条验证命令（`.peck/verify.toml`）。
    #[serde(rename = "verify.started")]
    VerifyStarted {
        check_id: String,
        name: String,
        command: String,
        trigger: VerifyTrigger,
        /// 第几次自动修之后跑的（0 是还没修过）。
        #[serde(default)]
        attempt: u32,
    },
    #[serde(rename = "verify.completed")]
    VerifyCompleted {
        check_id: String,
        name: String,
        status: VerifyStatus,
        #[serde(default)]
        exit_code: Option<i32>,
        #[serde(default)]
        duration_ms: Option<u64>,
        /// 输出的结尾（完整的落盘，路径写在里面）。
        #[serde(default)]
        output: Option<String>,
    },
    /// 模型停下以后的一轮验证：都过了、交回去修、停下。
    #[serde(rename = "verify.round")]
    VerifyRound {
        attempt: u32,
        max_attempts: u32,
        outcome: VerifyOutcome,
        #[serde(default)]
        reason: Option<String>,
        /// 目标模式。
        #[serde(default)]
        goal: bool,
    },
    /// 后台命令（模型起的、常驻的 dev server）的状态变了。
    #[serde(rename = "background.updated")]
    BackgroundUpdated {
        id: String,
        name: String,
        command: String,
        kind: BackgroundKind,
        status: BackgroundStatus,
        #[serde(default)]
        exit_code: Option<i32>,
    },

    // ── 其他 ──
    #[serde(rename = "notice")]
    Notice { level: NoticeLevel, text: String },
    #[serde(rename = "retry")]
    Retry {
        attempt: u32,
        delay_ms: u64,
        reason: String,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(default)]
        fatal: bool,
    },
    /// 不认识的事件：原件在事件日志的 `raw` 列。`kind` 是原件自己的类型名（有的话）。
    #[serde(rename = "raw")]
    Raw {
        source: String,
        #[serde(default)]
        kind: Option<String>,
    },
}

fn yes() -> bool {
    true
}

impl EventBody {
    /// 事件的类型名，和 JSON 里的 `type` 一样（事件日志的 `type` 列用它）。
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "session.started",
            Self::SessionConfigChanged { .. } => "session.config_changed",
            Self::TurnStarted { .. } => "turn.started",
            Self::TurnCompleted { .. } => "turn.completed",
            Self::MessageDelta { .. } => "message.delta",
            Self::MessageCompleted { .. } => "message.completed",
            Self::ReasoningDelta { .. } => "reasoning.delta",
            Self::ReasoningCompleted { .. } => "reasoning.completed",
            Self::ToolCallStarted { .. } => "tool_call.started",
            Self::ToolCallOutputDelta { .. } => "tool_call.output_delta",
            Self::ToolCallCompleted { .. } => "tool_call.completed",
            Self::DiffUpdated { .. } => "diff.updated",
            Self::PlanUpdated { .. } => "plan.updated",
            Self::ApprovalRequested { .. } => "approval.requested",
            Self::ApprovalResolved { .. } => "approval.resolved",
            Self::UserInputRequested { .. } => "user_input.requested",
            Self::UserInputResolved { .. } => "user_input.resolved",
            Self::SubagentStarted { .. } => "subagent.started",
            Self::SubagentCompleted { .. } => "subagent.completed",
            Self::UsageUpdated { .. } => "usage.updated",
            Self::QuotaUpdated { .. } => "quota.updated",
            Self::ContextUpdated { .. } => "context.updated",
            Self::ContextCompacted { .. } => "context.compacted",
            Self::MemoryProposed { .. } => "memory.proposed",
            Self::MemoryReviewed { .. } => "memory.reviewed",
            Self::VerifyStarted { .. } => "verify.started",
            Self::VerifyCompleted { .. } => "verify.completed",
            Self::VerifyRound { .. } => "verify.round",
            Self::BackgroundUpdated { .. } => "background.updated",
            Self::Notice { .. } => "notice",
            Self::Retry { .. } => "retry",
            Self::Error { .. } => "error",
            Self::Raw { .. } => "raw",
        }
    }

    /// 全部类型名（给 TS 对照测试和文档用）。
    pub const TYPE_NAMES: [&'static str; 33] = [
        "session.started",
        "session.config_changed",
        "turn.started",
        "turn.completed",
        "message.delta",
        "message.completed",
        "reasoning.delta",
        "reasoning.completed",
        "tool_call.started",
        "tool_call.output_delta",
        "tool_call.completed",
        "diff.updated",
        "plan.updated",
        "approval.requested",
        "approval.resolved",
        "user_input.requested",
        "user_input.resolved",
        "subagent.started",
        "subagent.completed",
        "usage.updated",
        "quota.updated",
        "context.updated",
        "context.compacted",
        "memory.proposed",
        "memory.reviewed",
        "verify.started",
        "verify.completed",
        "verify.round",
        "background.updated",
        "notice",
        "retry",
        "error",
        "raw",
    ];
}

/// 上下文里的一块：`system`（Peck 的说明和工具）、`project`（项目说明和记忆）、
/// `conversation`（对话和思考）、`tools`（工具的输出）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPart {
    pub key: String,
    pub tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactStep {
    /// 长的工具输出落盘，上下文里只留开头结尾和路径。
    Spill,
    /// 旧的工具输出清掉（只留「做过什么」）。
    Clear,
    /// 模型写一份交接摘要，前面的对话换成它。
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyTrigger {
    /// 模型改了文件，后台先跑一遍。
    Edit,
    /// 模型停下了，收尾前跑。
    End,
    /// 目标模式：每次停下都跑目标那几条。
    Goal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyStatus {
    Passed,
    Failed,
    /// 跑的时候文件又变了，结果不算（会再跑）。
    Stale,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyOutcome {
    /// 都过了：不再叫模型。
    Passed,
    /// 没过：失败的输出交回模型修。
    Fixing,
    /// 停下：修够了次数、同一个失败连续两次、两次之间没改文件。
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundKind {
    /// 模型用 `bash_background` 起的。
    Command,
    /// `.peck/verify.toml` 里的常驻 dev server。
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundStatus {
    Running,
    Exited,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    /// `connected`、`failed`、`pending` 这类，照引擎报的原样。
    pub status: String,
}

/// 权限三档。和 `src/lib/types.ts` 的 `Permission` 一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Plan,
    Edit,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    /// 做完了。
    Done,
    /// 到上限（轮数、上下文、花费、最大输出）。
    Limit,
    /// 模型拒绝。
    Refused,
    /// 站主停的。
    Cancelled,
    /// 站主打断（中途插话、换方向）。
    Interrupted,
    Error,
}

/// 线协议。思考只在「供应商 + 线协议 + 模型」都一样时回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    /// Anthropic Messages（Anthropic 和 Vertex 上的 Claude）。
    Messages,
    /// OpenAI Responses。
    Responses,
    /// Chat Completions（DeepSeek、百炼各有方言）。
    Chat,
    /// Gemini 原生 `streamGenerateContent`（Gemini 和 Vertex 上的 Gemini）。
    Gemini,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    /// 供应商 id（`anthropic`、`vertex-anthropic`……）。
    pub provider: String,
    pub wire: Wire,
    /// 发给供应商的模型名。
    pub model: String,
}

impl Origin {
    /// 思考的私有数据能不能原样发给 `target`：三样都一样才行，换了就丢。
    pub fn can_replay_to(&self, target: &Origin) -> bool {
        self == target
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueReasoning {
    pub origin: Origin,
    /// 原样的数据：Anthropic 的 `signature` / `redacted_thinking`、OpenAI 的 `encrypted_content`、
    /// Gemini 的 `thoughtSignature`、DeepSeek 和百炼的 `reasoning_content`。
    pub data: Value,
}

/// 读、改、跑、搜、取网页、MCP、其他。界面按它挑图标和折叠的说法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Run,
    Search,
    Fetch,
    Mcp,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Ok,
    Failed,
    /// 站主或规则拒了。
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    /// 相对项目根的路径。
    pub path: String,
    pub change: FileChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
    /// unified diff 原文（引擎给了才有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

/// 审批选项统一成四个：这次允许、总是允许、这次拒绝、总是拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    AllowAlways,
    DenyOnce,
    DenyAlways,
}

impl ApprovalDecision {
    pub fn allows(self) -> bool {
        matches!(self, Self::AllowOnce | Self::AllowAlways)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedBy {
    /// 站主点的。
    User,
    /// Peck 的规则（「这个项目总是允许」、档位）。
    Rule,
    /// 引擎自己定的（预先配置的权限）。
    Engine,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub input: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub cache_read: Option<u64>,
    #[serde(default)]
    pub cache_write: Option<u64>,
    #[serde(default)]
    pub reasoning: Option<u64>,
    /// 现在上下文里有多少 tokens（能报的引擎才有）。
    #[serde(default)]
    pub context_used: Option<u64>,
    #[serde(default)]
    pub context_window: Option<u64>,
}

impl TokenUsage {
    /// 整个提示的 tokens：没走缓存的 + 缓存读 + 缓存写。
    pub fn prompt_total(&self) -> u64 {
        self.input.unwrap_or(0) + self.cache_read.unwrap_or(0) + self.cache_write.unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    /// 美元。来源是 `unknown` 时为空，不填 0。
    #[serde(default)]
    pub usd: Option<f64>,
    pub source: CostSource,
}

impl Cost {
    pub fn unknown() -> Self {
        Self {
            usd: None,
            source: CostSource::Unknown,
        }
    }
}

/// 花费从哪来：供应商实报、按价格表算、CLI 自己估的、不知道。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    Reported,
    Priced,
    Estimated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageScope {
    /// 这一次请求或这一回合。
    Turn,
    /// 任务到现在的累计（Claude Code 续接以后报的是累计）。
    Cumulative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// 「5 小时」「每周」这类，照引擎报的。
    pub label: String,
    #[serde(default)]
    pub used_percent: Option<f64>,
    /// 毫秒时间戳。
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Warn,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_round_trip_with_dotted_type_and_camel_fields() {
        let event = AgentEvent {
            seq: 3,
            at: 1_700_000_000_000,
            subagent_id: None,
            body: EventBody::ToolCallCompleted {
                call_id: "c1".into(),
                status: ToolStatus::Ok,
                output: Some("ok".into()),
                exit_code: Some(0),
                diff: vec![],
                duration_ms: Some(12),
            },
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({"seq":3,"at":1_700_000_000_000i64,"type":"tool_call.completed","callId":"c1",
                   "status":"ok","output":"ok","exitCode":0,"durationMs":12})
        );
        assert_eq!(serde_json::from_value::<AgentEvent>(value).unwrap(), event);
    }

    #[test]
    fn type_names_match_serialized_tags() {
        let samples = vec![
            EventBody::TurnStarted { turn_id: "t".into() },
            EventBody::Notice {
                level: NoticeLevel::Info,
                text: "x".into(),
            },
            EventBody::Raw {
                source: "codex".into(),
                kind: None,
            },
            EventBody::UsageUpdated {
                usage: TokenUsage::default(),
                cost: Cost::unknown(),
                scope: UsageScope::Turn,
            },
        ];
        for body in samples {
            let value = serde_json::to_value(&body).unwrap();
            assert_eq!(value["type"], body.type_name());
            assert!(EventBody::TYPE_NAMES.contains(&body.type_name()));
        }
    }

    #[test]
    fn reasoning_replays_only_to_the_same_origin() {
        let a = Origin {
            provider: "anthropic".into(),
            wire: Wire::Messages,
            model: "claude-opus-5-5".into(),
        };
        let mut b = a.clone();
        assert!(a.can_replay_to(&b));
        b.provider = "vertex-anthropic".into();
        assert!(!a.can_replay_to(&b));
        let mut c = a.clone();
        c.model = "claude-sonnet-5-5".into();
        assert!(!a.can_replay_to(&c));
    }

    #[test]
    fn ts_mirror_lists_every_type() {
        let ts = include_str!("../../../src/lib/agentEvents.ts");
        for name in EventBody::TYPE_NAMES {
            assert!(
                ts.contains(&format!("\"{name}\"")),
                "agentEvents.ts 里少了 {name}"
            );
        }
    }
}
