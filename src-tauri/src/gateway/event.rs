//! 统一事件：Peck Loop 和每个 CLI 引擎都转成这一套，界面只认它。
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
/// 读回来手写（见下面的 `Deserialize`）：`subagent.started` / `completed` 自己的 `subagentId` 和外层的同名。
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// 外层的 `subagentId` 和 `subagent.started` / `completed` 里的同名：派生出来的会让外层先拿走，事件那一半就缺了它，
/// 存下的这两种事件读回来全成了 `raw`（重开任务时 LOG 里派了谁、子任务那一行的头都没了；v1.1.3 修）。
/// 这两种的 `subagentId` 归事件本身（它们一律从主线发，外层没有）；JSON 里万一有两个，后一个算。
impl<'de> Deserialize<'de> for AgentEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut map = serde_json::Map::<String, Value>::deserialize(d)?;
        let seq = map.remove("seq").and_then(|v| v.as_u64()).ok_or_else(|| D::Error::missing_field("seq"))?;
        let at = map.remove("at").and_then(|v| v.as_i64()).ok_or_else(|| D::Error::missing_field("at"))?;
        let own = matches!(map.get("type").and_then(Value::as_str), Some("subagent.started" | "subagent.completed"));
        let subagent_id = if own { None } else { map.remove("subagentId").and_then(|v| v.as_str().map(str::to_string)) };
        let body = EventBody::deserialize(Value::Object(map)).map_err(D::Error::custom)?;
        Ok(AgentEvent { seq, at, subagent_id, body })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum EventBody {
    // ── 会话 ──
    /// 引擎起来了：它是谁、能做什么、带了哪些工具和 MCP。
    #[serde(rename = "session.started")]
    SessionStarted {
        /// 引擎 id：`peck`（Peck Loop）、`codex`、`claude-code`、`muse`、`grok`、`antigravity`。
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
        /// Peck 派出去的子代理（v0.9 起）：用的那个 API 模型。CLI 自己的子代理（Claude Code 的 Task）为空。
        /// 按段列花费按它写是谁。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// 子代理的类型（v1.1.2）：explorer / searcher / worker / member / auditor；以前的委派、CLI 的为空。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    /// 这个任务的成员名单（v1.1.2，PLOOP 开头发一次）：主角色和能派的几类子代理各用哪个模型。界面放在 LOG 里，
    /// 不进系统提示（换了设置不让提示缓存失效）。
    #[serde(rename = "agents.roster")]
    AgentsRoster { members: Vec<RosterMember> },
    /// 任务板（v1.1.3，整份，后一条替掉前一条）：主角色派出去的 worker、member 各一项。进度（排队、在跑、等你批、
    /// 做完）由 Peck 照子代理的事件填，模型不改。主角色自己那几步还是 `plan.updated`。
    #[serde(rename = "board.updated")]
    BoardUpdated { items: Vec<BoardItem> },
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
        /// 哪个模型花的（v0.9）：Peck Loop 填目录里的模型 id（委派的子任务是子任务的模型），QUOTA 按它记；
        /// CLI 引擎不填（按引擎记）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
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
    /// 模型提议存一条 Skill（v0.10 C3）：只是提议，站主在卡片上批了才写盘。不打断任务，跑着、跑完都能批。
    #[serde(rename = "skill.proposed")]
    SkillProposed {
        proposal_id: String,
        name: String,
        description: String,
        body: String,
        /// `project`（项目的 `.claude/skills`）、`project-skills`（和 AGENTS.md 同级的 `skills/`）、`user`（Peck 的全局 Skill，数据目录的 `skills/`）。
        scope: String,
        /// 给人看的位置：`.claude/skills/<名字>/SKILL.md`、`Peck/skills/<名字>/SKILL.md`（全局）。
        location: String,
        /// 同名的已经有了：这次是替换，`diff` 是旧的 → 新的（统一格式）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
        /// 标黄提醒的（绝对路径、邮箱这类）。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
        /// 名字、描述像的已有 Skill（可能重复）。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        similar: Vec<String>,
    },
    /// 站主批了一条 Skill 提议：`saved` 写好了、`queued` 批了但任务还在跑（做完再写，免得算进这次的改动）、
    /// `skipped` 不要、`failed` 没写进去（`error`）。同一条可能先 `queued` 再 `saved`。
    #[serde(rename = "skill.reviewed")]
    SkillReviewed {
        proposal_id: String,
        status: SkillReviewStatus,
        /// 最后用的名字和位置（站主可能在卡片上改过）。
        name: String,
        location: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// 插件的收尾 Hook 跑完了（v1.1.0，见 `harness/hooks.rs`）：Git Plan 提交了、或者为什么没提交。`text` 是给人看的一句。
    #[serde(rename = "hook.ran")]
    HookRan {
        plugin: String,
        action: String,
        ok: bool,
        text: String,
        /// 脚本 Hook（v1.1.2）是哪个时刻的（`PreToolUse` …）：界面写在过程里当时那一处。收尾提交的为空，写在 recap 里。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },

    // ── 后台（v0.6）──
    /// 后台命令（模型起的；标了留着的 dev server）的状态变了。
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

    // ── 合作（v0.8）──
    /// 接力、审查里的一段开始了：哪一段、谁来干。这一段的事件跟在后面，收尾是 `stage.completed`
    /// （这一段引擎自己的 `turn.completed` 换成它；整个任务最后才有一条 `turn.completed`）。
    #[serde(rename = "stage.started")]
    StageStarted {
        stage: StageRole,
        /// 引擎 id（`peck`、`codex`……）。
        engine: String,
        #[serde(default)]
        model: Option<String>,
        /// 给人看的：「Codex 写规范」。
        title: String,
    },
    #[serde(rename = "stage.completed")]
    StageCompleted {
        stage: StageRole,
        reason: TurnEndReason,
        #[serde(default)]
        error: Option<String>,
        /// 审查的结论（只有审查那一段有）。
        #[serde(default)]
        verdict: Option<ReviewVerdict>,
        /// 审查按规范的验收标准逐条判的结果（v0.9，接力带了验收标准才有）。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        checks: Vec<CriterionCheck>,
    },

    // ── 其他 ──
    #[serde(rename = "notice")]
    Notice { level: NoticeLevel, text: String },
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
            Self::AgentsRoster { .. } => "agents.roster",
            Self::BoardUpdated { .. } => "board.updated",
            Self::UsageUpdated { .. } => "usage.updated",
            Self::QuotaUpdated { .. } => "quota.updated",
            Self::ContextUpdated { .. } => "context.updated",
            Self::ContextCompacted { .. } => "context.compacted",
            Self::MemoryProposed { .. } => "memory.proposed",
            Self::MemoryReviewed { .. } => "memory.reviewed",
            Self::SkillProposed { .. } => "skill.proposed",
            Self::SkillReviewed { .. } => "skill.reviewed",
            Self::HookRan { .. } => "hook.ran",
            Self::BackgroundUpdated { .. } => "background.updated",
            Self::StageStarted { .. } => "stage.started",
            Self::StageCompleted { .. } => "stage.completed",
            Self::Notice { .. } => "notice",
            Self::Error { .. } => "error",
            Self::Raw { .. } => "raw",
        }
    }

    /// 全部类型名（给 TS 对照测试和文档用）。
    #[cfg(test)]
    pub const TYPE_NAMES: [&'static str; 36] = [
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
        "agents.roster",
        "board.updated",
        "usage.updated",
        "quota.updated",
        "context.updated",
        "context.compacted",
        "memory.proposed",
        "memory.reviewed",
        "skill.proposed",
        "skill.reviewed",
        "hook.ran",
        "background.updated",
        "stage.started",
        "stage.completed",
        "notice",
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

/// 接力、审查里的一段（v0.8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageRole {
    /// 接力的前一个：只读，写一份实施规范。
    Spec,
    /// 动手的那一个。
    Implement,
    /// 审查：只读，看这次的改动。
    Review,
    /// 审查说要改：交回动手的那一个修一次。
    Fix,
}

/// 审查的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    Changes,
}

/// 一条验收标准的审查结果（v0.9）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CriterionCheck {
    /// 规范里的编号（从 1 起）。
    pub n: u32,
    pub criterion: String,
    /// 通过、没过；审查没判这一条为空。
    #[serde(default)]
    pub pass: Option<bool>,
    /// 审查写的理由（没过的写哪里不对、怎么改）。
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundKind {
    /// 模型用 `bash_background` 起的。
    Command,
    /// 模型起的时候标了留着的 dev server：任务结束不停，退出 Peck 才停。
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundStatus {
    Running,
    Exited,
    Stopped,
}

/// 任务板上的一项（v1.1.3）：派出去的一个 worker 或 member。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardItem {
    /// 派它的那一步的调用 id（= 它事件里的 `subagentId`，界面点开看它的步骤）。
    pub id: String,
    pub title: String,
    /// `worker` / `member`。
    pub owner: String,
    /// 这一类派的第几个（「worker 2」）。
    pub number: u32,
    /// 它能改的文件、文件夹（相对项目根）。
    pub paths: Vec<String>,
    pub status: BoardStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 真开始跑（排完队）、做完的时刻（毫秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardStatus {
    /// 排队：同类到了同时几个的上限，或者要改的和前面的重叠。
    Queued,
    Running,
    /// 等站主批（它的审批卡片还没答）。
    Waiting,
    Done,
    Failed,
    Stopped,
}

/// 名单上的一个：`main` 是主角色，别的是子代理的类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterMember {
    pub role: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
    /// 同时能跑几个（explorer、searcher）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<u32>,
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
    /// 站主停的、打断的。
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
    /// 联网搜索了几次（v1.1.1）：供应商自己的搜索按次收费，Peck 的 `web_search`（DeepSeek、Tavily）也按次记。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_searches: Option<u64>,
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

/// 一条 Skill 提议批成什么样了（`skill.reviewed`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillReviewStatus {
    Saved,
    Queued,
    Skipped,
    Failed,
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
        // 子代理开始、做完：自己的 subagentId 读回来还在（以前读回来成了 raw）；子任务里的事件外层照旧带。
        let started = AgentEvent { seq: 4, at: 1, subagent_id: None, body: EventBody::SubagentStarted { subagent_id: "c9".into(), parent_call_id: Some("c9".into()), title: "worker · x".into(), model: None, kind: Some("worker".into()) } };
        let text = serde_json::to_string(&started).unwrap();
        assert_eq!(serde_json::from_str::<AgentEvent>(&text).unwrap(), started);
        let old = r#"{"seq":1747,"at":1,"type":"subagent.completed","subagentId":"peck-sub-1","status":"failed"}"#;
        assert!(matches!(serde_json::from_str::<AgentEvent>(old).unwrap().body, EventBody::SubagentCompleted { subagent_id, .. } if subagent_id == "peck-sub-1"));
        let inner = AgentEvent { seq: 5, at: 1, subagent_id: Some("c9".into()), body: EventBody::Notice { level: NoticeLevel::Info, text: "x".into() } };
        assert_eq!(serde_json::from_str::<AgentEvent>(&serde_json::to_string(&inner).unwrap()).unwrap(), inner);
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
                model: None,
            },
            EventBody::BoardUpdated {
                items: vec![BoardItem { id: "c1".into(), title: "改 a".into(), owner: "worker".into(), number: 1, paths: vec!["src/a.rs".into()], status: BoardStatus::Queued, model: None, started_at: None, finished_at: None }],
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
