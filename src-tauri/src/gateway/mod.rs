//! Model Gateway：各家 API 和 CLI 的统一抽象层（v0.5 Phase 5，本地开发）。
//!
//! 两个接口：
//! - [`ModelProvider`]：裸模型。给对话和工具定义，流式回字、回思考、回要调用的工具；
//!   工具由调用方（Peck 自己的循环，`harness/`）执行。
//! - [`AgentBackend`]：整包引擎（Codex、Claude Code、Muse、Grok、Antigravity）。给一句话，
//!   引擎自己读写跑；过程转成统一事件（[`event`]），要批的事、要问的事走控制通道（[`Control`]）。
//!
//! Peck 自己的循环（云端 Phase 6 的 `harness/`）也实现 [`AgentBackend`]：里面用 [`ModelProvider`]
//! 调模型、自己跑工具，对外出同一套事件。这样界面只认一套。
//!
//! 这一层不认识界面，也不认识任务表；事件写进事件日志和发给界面由 [`log::EventSink`] 做。

pub mod accounts;
pub mod caps;
pub mod catalog;
pub mod cli_models;
pub mod config;
pub mod convert;
pub mod engines;
pub mod event;
pub mod log;
pub mod mcp;
pub mod mock;
pub mod providers;
pub mod runner;

use event::{
    ApprovalDecision, Cost, OpaqueReasoning, PermissionLevel, TokenUsage, TurnEndReason, Wire,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::sync::{mpsc, watch};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

// ───────────────────────── 裸模型 ─────────────────────────

/// 一家 API 供应商（Anthropic、OpenAI、DeepSeek、百炼、Gemini、Vertex 上的 Claude 和 Gemini）。
pub trait ModelProvider: Send + Sync {
    /// 供应商 id，和 `defaults/models.json` 的 `providers[].id` 一样。
    fn id(&self) -> &str;
    fn wire(&self) -> Wire;
    /// 发一次请求。增量走 `events`；返回整条回答。`cancel` 变成 `true` 就尽快停，返回 `Cancelled`。
    /// 失败不自动重发（调用方决定）；可重试的错误在 `GatewayError::retryable` 标出来。
    fn complete(
        &self,
        request: ModelRequest,
        events: mpsc::UnboundedSender<ModelEvent>,
        cancel: watch::Receiver<bool>,
    ) -> BoxFuture<Result<ModelResponse, GatewayError>>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    /// 目录里的模型 id（`anthropic/claude-opus-5-5`），不是发给供应商的名字。
    pub model: String,
    /// 系统提示，按块给；`cache` 为真的块后面打缓存断点（Anthropic 这类要显式打的才用得上）。
    #[serde(default)]
    pub system: Vec<SystemBlock>,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// 开不开思考。模型只能开着的（`thinking: always`）忽略 `false`。
    #[serde(default)]
    pub thinking: bool,
    /// 思考强度（`low`、`high`……），取值看目录里这个模型的 `efforts`。
    #[serde(default)]
    pub effort: Option<String>,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemBlock {
    pub text: String,
    #[serde(default)]
    pub cache: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema。
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// 和供应商无关的一条消息。转成各家格式由各家的 provider 做。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Part {
    Text {
        text: String,
    },
    /// 思考。回放时 provider 先看 `opaque.origin` 能不能发给这一家，不能就整块丢掉。
    Reasoning {
        text: String,
        #[serde(default)]
        opaque: Option<OpaqueReasoning>,
    },
    /// 模型要调用的工具。`signature` 是 Gemini 挂在调用上的 thought signature，回放时必须带上。
    ToolCall {
        call_id: String,
        name: String,
        input: Value,
        #[serde(default)]
        signature: Option<OpaqueReasoning>,
    },
    ToolResult {
        call_id: String,
        output: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// 流式增量。整条回答在 [`ModelResponse`] 里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ModelEvent {
    TextDelta { text: String },
    ReasoningDelta { text: String },
    /// 模型开始写一个工具调用（参数还没写完）。界面可以先露出「要读文件……」。
    ToolCallStarted { call_id: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    /// 助手这一条：思考（带私有数据）、文字、工具调用，按出现的顺序。下一轮原样放回 `messages`。
    pub message: Message,
    pub stop: StopReason,
    pub usage: TokenUsage,
    pub cost: Cost,
    /// 供应商回报的实际模型名。
    #[serde(default)]
    pub response_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal,
    Other(String),
}

// ───────────────────────── 整包引擎 ─────────────────────────

/// 一个 CLI 引擎（或 Peck 自己的循环）。一次 `run` 是一个任务的一个回合（续接时带 `resume`）。
pub trait AgentBackend: Send + Sync {
    /// 引擎 id：`peck`、`codex`、`claude-code`、`muse`、`grok`、`antigravity`。
    fn id(&self) -> &str;
    fn caps(&self) -> caps::AgentCaps;
    /// 跑一个回合。事件全部经 `sink` 发出（第一条是 `session.started`，最后一条是 `turn.completed`）；
    /// `control` 收站主的审批、回答、追加、打断。返回结束原因和续接用的会话号。
    fn run(
        &self,
        task: AgentTask,
        sink: log::EventSink,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Result<RunOutcome, GatewayError>>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    /// 任务（run）id，事件日志按它存。
    pub run_id: String,
    pub prompt: String,
    /// 项目根。
    pub cwd: PathBuf,
    pub permission: PermissionLevel,
    /// 引擎里的模型名（不填用引擎默认）。
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// 上一次 `RunOutcome::session_ref`，续接用。
    #[serde(default)]
    pub resume: Option<String>,
    /// Peck 的本机 MCP 入口（审批、ASK）。没起来就为空，引擎照常跑、ASK 不可用。
    #[serde(default)]
    pub peck_mcp: Option<PeckMcp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeckMcp {
    /// `http://127.0.0.1:<port>/mcp`
    pub url: String,
    /// 每个任务一个随机令牌。不进事件、不进日志。
    #[serde(skip_serializing)]
    pub token: String,
}

/// 控制通道：界面 → 引擎。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Control {
    /// 回答 `approval.requested`。
    Approve {
        request_id: String,
        decision: ApprovalDecision,
    },
    /// 回答 `user_input.requested`。
    Answer { request_id: String, answer: String },
    /// 引擎干活时追加一句（`caps.steer` 为真才发）。
    Append { text: String },
    /// 停下当前回合（先发中断、等它收尾，超时再结束进程组）。
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    pub reason: TurnEndReason,
    #[serde(default)]
    pub session_ref: Option<String>,
}

// ───────────────────────── 错误 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayError {
    pub kind: ErrorKind,
    /// 给站主看的一句中文。不带 Key、令牌、绝对路径。
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 没配 Key、Key 不对、ADC 过期、CLI 没登录。
    Auth,
    RateLimit,
    Network,
    /// 请求被拒（400 这类，多半是格式问题）。
    BadRequest,
    /// 供应商那边出错（5xx、过载）。
    Upstream,
    Cancelled,
    /// CLI 没装、起不来、中途退出。
    Engine,
    Internal,
}

impl GatewayError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        let retryable = matches!(
            kind,
            ErrorKind::RateLimit | ErrorKind::Network | ErrorKind::Upstream
        );
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "已停止。")
    }
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GatewayError {}

// ───────────────────────── IPC ─────────────────────────

/// 模型列表：Peck（API 模型，按供应商）+ CLI 引擎 + 现有的接力和规划 + 实施。
#[tauri::command]
pub async fn gateway_models(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<catalog::ModelEntry>, String> {
    let mock = *state.api.mode.lock().map_err(|_| "读不到 API 模式。")? == crate::mode::ApiMode::Mock;
    let keys: Vec<(String, bool)> = state
        .api
        .keys
        .lock()
        .map_err(|_| "读不到 Key 状态。")?
        .status()
        .into_iter()
        .map(|k| (k.provider_id, k.state == "saved"))
        .collect();
    let config = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        config::GatewayConfig::load(&store).map_err(|e| e.to_string())?
    };
    let engines = if mock {
        catalog::EngineProbe::all_found()
    } else {
        catalog::EngineProbe::detect(&config).await
    };
    let adc = if mock {
        true
    } else {
        tokio::task::spawn_blocking(config::adc_present)
            .await
            .unwrap_or(false)
    };
    let book = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        accounts::AccountBook::load(&store)
    };
    let mut entries = catalog::model_entries(
        catalog::catalog(),
        &catalog::Availability {
            all: mock,
            keys,
            vertex_ready: adc && config.vertex.is_set(),
            vertex_adc: adc,
            engines,
        },
    );
    // CLI 引擎后面显示当前账号（默认账号不写）。
    for entry in &mut entries {
        if let Some(engine) = entry.id.strip_prefix("cli:") {
            entry.account = accounts::current_label(&book, engine);
        }
    }
    Ok(entries)
}

/// 设置页用：Gateway 的设置，和 gcloud 的 ADC 在不在（只看文件，不读内容）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySettings {
    pub config: config::GatewayConfig,
    pub adc: bool,
}

#[tauri::command]
pub async fn gateway_config_get(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<GatewaySettings, String> {
    let config = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        config::GatewayConfig::load(&store).map_err(|e| e.to_string())?
    };
    let adc = tokio::task::spawn_blocking(config::adc_present)
        .await
        .unwrap_or(false);
    Ok(GatewaySettings { config, adc })
}

#[tauri::command]
pub async fn gateway_config_set(
    config: config::GatewayConfig,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<config::GatewayConfig, String> {
    let config = config.normalized()?;
    let store = state.store.lock().map_err(|_| "存不了设置。")?;
    config.save(&store).map_err(|e| e.to_string())?;
    Ok(config)
}

/// 一个任务的事件日志（按序号）。旧任务没有事件，返回空。
#[tauri::command]
pub async fn gateway_events(
    run_id: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<event::AgentEvent>, String> {
    let store = state.store.lock().map_err(|_| "读不到事件。")?;
    store.agent_events(&run_id).map_err(|e| e.to_string())
}
