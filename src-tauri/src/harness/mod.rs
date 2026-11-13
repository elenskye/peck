//! Peck 自己的循环（v0.5 Phase 6，云端开发）：通过 Gateway 的 `ModelProvider` 调任何一家 API 模型，
//! 带自己的工具箱，产出和 CLI 引擎同一套统一事件（实现 `AgentBackend`，引擎 id `peck`）。
//! v0.6：上下文（`context`、`compact`、`history`）、后台和验证（`background`、`verify`、`monitor`）。

pub mod background;
pub mod compact;
pub mod context;
pub mod history;
pub mod monitor;
pub mod permission;
pub mod risk;
pub mod spill;
pub mod tools;
pub mod r#loop;
pub mod run;
pub mod verify;
