use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StepDescriptor {
    pub id: String,
    pub step_index: usize,
    pub agent_id: String,
    pub agent_name: String,
    pub title: String,
    pub status: StepStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeItem {
    pub path: String,
    pub kind: String, // "create" | "modify" | "delete"
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetSummary {
    pub files: Vec<FileChangeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    RunStarted {
        run_id: String,
        steps: Vec<StepDescriptor>,
    },
    #[serde(rename_all = "camelCase")]
    StepStarted {
        run_id: String,
        step_id: String,
        step_index: usize,
        agent_id: String,
    },
    #[serde(rename_all = "camelCase")]
    StepFinished {
        run_id: String,
        step_id: String,
        status: StepStatus,
        exit_code: Option<i32>,
        duration_ms: u64,
        changeset: ChangeSetSummary,
    },
    #[serde(rename_all = "camelCase")]
    RunFinished {
        run_id: String,
        status: RunStatus,
        summary: String,
    },
}

/// 把任务记录的变化（开始、那一步开始和做完、收尾）写进 `runs` 那一行。存不进去只在终端里记一笔。
#[derive(Clone)]
pub struct EventSink {
    store: Arc<Mutex<crate::store::Store>>,
}

impl EventSink {
    pub fn new(store: Arc<Mutex<crate::store::Store>>) -> Self {
        Self { store }
    }

    pub fn emit(&self, event: RunEvent) {
        let result = self.store.lock().map_err(|e| e.to_string()).and_then(|mut db| db.record_event(&event).map_err(|e| e.to_string()));
        if let Err(error) = result {
            eprintln!("Peck 历史保存失败: {error}");
        }
    }
}
