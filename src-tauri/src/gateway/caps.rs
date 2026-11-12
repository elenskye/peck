//! 能力标记：界面按它降级（不能打断就不给「打断」，没有实时审批就写明拦不住……）。
//!
//! 各家的初值照 `docs/v0.5.md`「CLI 五家」那张表；标「实测」的项 Phase 5 真跑以后再改。

use super::event::CostSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCaps {
    /// 能不能边出字边显示。
    pub streaming: bool,
    /// 思考能不能原样回放给下一轮（签名、密文）。
    pub reasoning_replay: bool,
    /// 工具归谁执行：Peck 自己的工具箱，还是引擎自带的。
    pub tools: ToolOwner,
    /// 审批：没有、预先配置（启动时定好）、实时（每次问 Peck）。
    pub approvals: ApprovalMode,
    /// 模型能不能问站主（ASK 卡片）。
    pub ask_user: bool,
    /// 能不能在它干活时追加一句话。
    pub steer: bool,
    /// 能不能打断（停下当前回合，不结束会话）。
    pub interrupt: bool,
    /// 能不能续接上一次的会话。
    pub resume: bool,
    /// 能不能把 Peck 的本机 MCP 加进去。
    pub mcp_inject: bool,
    /// 用量多细：每轮、只有累计、没有。
    pub usage: UsageGranularity,
    /// 花费从哪来。
    pub cost: CostSource,
    /// 有没有订阅额度可读。
    pub quota: bool,
    /// 会话中途能不能换模型。
    pub switch_model: bool,
    /// 要不要给它一个伪终端（有的 CLI 非终端下会挂起）。
    pub needs_tty: bool,
    /// 只做直接调用：不进 ONE SHOT、接力、委派、审查、定时任务、自动修（Antigravity）。
    pub direct_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOwner {
    Peck,
    Engine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    None,
    Preset,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageGranularity {
    PerTurn,
    Cumulative,
    None,
}

impl AgentCaps {
    /// Peck 自己的循环 + 一家 API 模型。
    pub fn peck_loop() -> Self {
        Self {
            streaming: true,
            reasoning_replay: true,
            tools: ToolOwner::Peck,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: true,
            interrupt: true,
            resume: true,
            mcp_inject: false,
            usage: UsageGranularity::PerTurn,
            cost: CostSource::Priced,
            quota: false,
            switch_model: true,
            needs_tty: false,
            direct_only: false,
        }
    }

    /// `codex app-server`（stdio JSON-RPC）。
    pub fn codex() -> Self {
        Self {
            streaming: true,
            reasoning_replay: false,
            tools: ToolOwner::Engine,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: true,
            interrupt: true,
            resume: true,
            mcp_inject: true,
            usage: UsageGranularity::Cumulative,
            cost: CostSource::Unknown,
            quota: true,
            switch_model: true,
            needs_tty: false,
            direct_only: false,
        }
    }

    /// `claude -p` stream-json 双向；审批走 `--permission-prompt-tool` 指向 Peck 的 MCP。
    pub fn claude_code() -> Self {
        Self {
            streaming: true,
            reasoning_replay: false,
            tools: ToolOwner::Engine,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: true,
            interrupt: true,
            resume: true,
            mcp_inject: true,
            usage: UsageGranularity::Cumulative,
            cost: CostSource::Estimated,
            quota: true,
            switch_model: false,
            needs_tty: false,
            direct_only: false,
        }
    }

    /// Muse 走会话协议（`muse serve`）：它的 LLM 审查员先看，拿不准的交给 Peck，所以审批是实时的。
    pub fn muse() -> Self {
        Self {
            streaming: true,
            reasoning_replay: false,
            tools: ToolOwner::Engine,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: true,
            interrupt: true,
            resume: true,
            mcp_inject: true,
            usage: UsageGranularity::Cumulative,
            cost: CostSource::Unknown,
            quota: true,
            switch_model: false,
            needs_tty: false,
            direct_only: false,
        }
    }

    /// ACP（`grok agent stdio`）。
    pub fn grok() -> Self {
        Self {
            streaming: true,
            reasoning_replay: false,
            tools: ToolOwner::Engine,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: false,
            interrupt: true,
            resume: true,
            mcp_inject: true,
            usage: UsageGranularity::Cumulative,
            cost: CostSource::Estimated,
            quota: true,
            switch_model: true,
            needs_tty: false,
            direct_only: false,
        }
    }

    /// ACP（`agy acp`）。只做直接调用（站主 2026-11-12 定）。
    pub fn antigravity() -> Self {
        Self {
            streaming: true,
            reasoning_replay: false,
            tools: ToolOwner::Engine,
            approvals: ApprovalMode::Live,
            ask_user: true,
            steer: false,
            interrupt: true,
            resume: false,
            mcp_inject: true,
            usage: UsageGranularity::Cumulative,
            cost: CostSource::Unknown,
            quota: false,
            switch_model: true,
            needs_tty: true,
            direct_only: true,
        }
    }

    /// 按引擎 id 取初值；不认识的给最保守的一套。
    pub fn for_engine(engine: &str) -> Self {
        match engine {
            "peck" => Self::peck_loop(),
            "codex" => Self::codex(),
            "claude-code" => Self::claude_code(),
            "muse" => Self::muse(),
            "grok" => Self::grok(),
            "antigravity" => Self::antigravity(),
            _ => Self {
                streaming: false,
                reasoning_replay: false,
                tools: ToolOwner::Engine,
                approvals: ApprovalMode::None,
                ask_user: false,
                steer: false,
                interrupt: false,
                resume: false,
                mcp_inject: false,
                usage: UsageGranularity::None,
                cost: CostSource::Unknown,
                quota: false,
                switch_model: false,
                needs_tty: false,
                direct_only: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_antigravity_is_direct_only() {
        for engine in ["peck", "codex", "claude-code", "muse", "grok"] {
            assert!(!AgentCaps::for_engine(engine).direct_only, "{engine}");
        }
        assert!(AgentCaps::for_engine("antigravity").direct_only);
    }

    #[test]
    fn muse_approvals_reach_peck_through_its_session_protocol() {
        assert_eq!(AgentCaps::muse().approvals, ApprovalMode::Live);
    }
}
