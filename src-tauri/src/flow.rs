//! 任务记录里的「计划」：一个任务有哪几步（v0.8 起新的发任务都只有一步；旧记录照旧读）。
//! 旧的流程清单（`flows/*.toml`）和按清单派发在 v0.8 删了。
use serde::{Deserialize, Serialize};

use crate::events::{StepDescriptor, StepStatus};

/// Concrete resolved execution plan produced by routing user input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchPlan {
    pub flow_id: String,
    pub flow_name: String,
    pub clean_input: String,
    pub steps: Vec<ResolvedStepPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStepPlan {
    pub id: String,
    pub step_index: usize,
    pub agent_id: String,
    pub agent_name: String,
    pub title: String,
    pub rendered_prompt: String,
    pub rendered_expects: Vec<String>,
}

impl ResolvedStepPlan {
    pub fn to_descriptor(&self, status: StepStatus) -> StepDescriptor {
        StepDescriptor {
            id: self.id.clone(),
            step_index: self.step_index,
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            title: self.title.clone(),
            status,
            expects: self.rendered_expects.clone(),
            exit_code: None,
            duration_ms: None,
            started_at: None,
            finished_at: None,
            execution_mode: None,
            notice: None,
        }
    }
}
