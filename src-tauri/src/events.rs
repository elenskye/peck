use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StepDescriptor {
    pub id: String,
    pub step_index: usize,
    pub agent_id: String,
    pub agent_name: String,
    pub title: String,
    pub status: StepStatus,
    pub expects: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub execution_mode: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    Stdout,
    Stderr,
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
pub struct TaskItem {
    pub id: String,
    pub text: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeItem {
    pub path: String,
    pub kind: String, // "create" | "modify" | "delete"
    pub additions: usize,
    pub deletions: usize,
    pub step_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSetSummary {
    pub files: Vec<FileChangeItem>,
    pub total_additions: usize,
    pub total_deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum RunEvent {
    #[serde(rename_all = "camelCase")]
    RunStarted {
        run_id: String,
        flow_id: String,
        flow_name: String,
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
    StepExecution {
        run_id: String,
        step_id: String,
        mode: String,
        notice: String,
    },
    #[serde(rename_all = "camelCase")]
    StepOutput {
        run_id: String,
        step_id: String,
        stream: StreamType,
        chunk: String,
    },
    #[serde(rename_all = "camelCase")]
    TasksUpdated {
        run_id: String,
        step_id: String,
        total: usize,
        completed: usize,
        items: Vec<TaskItem>,
    },
    #[serde(rename_all = "camelCase")]
    FileTouched {
        run_id: String,
        step_id: String,
        path: String,
        kind: String,
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

/// Unified event sink bridging `RunEvent` streams to either a `tauri::ipc::Channel<RunEvent>`
/// or an in-memory collector (for tests and internal inspection).
#[derive(Clone)]
pub enum EventSink {
    TauriChannel(Channel<RunEvent>),
    Memory(Arc<Mutex<Vec<RunEvent>>>),
    Persistent {
        target: Box<EventSink>,
        store: Arc<Mutex<crate::store::Store>>,
    },
}

impl EventSink {
    pub fn from_channel(channel: Channel<RunEvent>) -> Self {
        Self::TauriChannel(channel)
    }

    pub fn new_memory() -> (Self, Arc<Mutex<Vec<RunEvent>>>) {
        let store = Arc::new(Mutex::new(Vec::new()));
        (Self::Memory(store.clone()), store)
    }

    pub fn emit(&self, event: RunEvent) {
        match self {
            Self::Persistent { target, store } => {
                let result = store
                    .lock()
                    .map_err(|e| e.to_string())
                    .and_then(|mut db| db.record_event(&event).map_err(|e| e.to_string()));
                if let Err(error) = result {
                    eprintln!("Peck 历史保存失败: {error}");
                    let value = serde_json::to_value(&event).unwrap_or_default();
                    target.emit(RunEvent::StepOutput {
                        run_id: value["payload"]["runId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        step_id: value["payload"]["stepId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        stream: StreamType::Stderr,
                        chunk: format!("历史保存失败：{error}"),
                    });
                }
                target.emit(event);
            }
            Self::TauriChannel(ch) => {
                let _ = ch.send(event);
            }
            Self::Memory(buf) => {
                if let Ok(mut guard) = buf.lock() {
                    guard.push(event);
                }
            }
        }
    }
}
