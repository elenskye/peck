//! Peck 自己的循环（v0.5 Phase 6，云端开发）：通过 Gateway 的 `ModelProvider` 调任何一家 API 模型，
//! 带自己的九个工具，产出和 CLI 引擎同一套统一事件（实现 `AgentBackend`，引擎 id `peck`）。

pub mod permission;
pub mod risk;
pub mod spill;
pub mod tools;
pub mod r#loop;
pub mod run;
