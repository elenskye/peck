use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::events::{
    FileChangeItem, RunEvent, RunStatus, StepDescriptor, StepStatus, StreamType, TaskItem,
};
use crate::flow::DispatchPlan;
use crate::snapshot::{saved_snapshot, RunSnapshotRecord};
use crate::workspace::Workspace;

pub type StoreResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub run_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawLogEntry {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub agent_id: String,
    pub stream: StreamType,
    pub chunk: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub number: u64,
    pub session_id: String,
    pub flow_id: String,
    pub flow_name: String,
    pub prompt: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub steps: Vec<StepDescriptor>,
    pub tasks: Vec<TaskItem>,
    pub files: Vec<FileChangeItem>,
    #[serde(default)]
    pub step_files: std::collections::HashMap<String, Vec<FileChangeItem>>,
    pub logs: Vec<RawLogEntry>,
    pub summary: Option<String>,
    /// 谁来干：`muse`、`grok`、`muse>grok`、`grok>muse`、`mock`；API 模式和旧记录没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    /// 发任务时选的权限档：`plan` / `edit` / `auto`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    /// 「撤销整个任务」的时间；没撤销过就没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_at: Option<i64>,
}

/// 左栏用的任务摘要：一次对话（session）一行，带最近一次记录的状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBrief {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub run_count: usize,
    pub last_run: Option<RunBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBrief {
    pub id: String,
    pub number: u64,
    pub flow_id: String,
    pub flow_name: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    #[serde(default)]
    pub files: Vec<FileChangeItem>,
    #[serde(default)]
    pub who: Option<String>,
    #[serde(default)]
    pub undone_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverview {
    pub workspace: Workspace,
    pub tasks: Vec<TaskBrief>,
}

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        crate::migrations::migrate(&mut conn, path)?;
        Ok(Self { conn })
    }

    pub fn save_workspace(&self, workspace: &Workspace) -> StoreResult<()> {
        self.conn.execute("INSERT INTO workspaces(id, data, last_opened_at) VALUES(?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET data=excluded.data, last_opened_at=excluded.last_opened_at",
            params![workspace.id, serde_json::to_string(workspace)?, workspace.last_opened_at])?;
        Ok(())
    }

    pub fn workspaces(&self) -> StoreResult<Vec<Workspace>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM workspaces ORDER BY last_opened_at DESC, rowid DESC")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|raw| Ok(serde_json::from_str(&raw)?))
            .collect()
    }

    pub fn workspace(&self, workspace_id: &str) -> StoreResult<Workspace> {
        let raw: String = self.conn.query_row(
            "SELECT data FROM workspaces WHERE id=?1",
            [workspace_id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// 界面设置：`key` 对应一段 JSON，没存过返回 `None`。
    pub fn setting(&self, key: &str) -> StoreResult<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM app_settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO app_settings(key, value, updated_at) VALUES(?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, now_ms()],
        )?;
        Ok(())
    }

    /// 左栏的项目列表：每个项目带它的任务（新的在上），只带最近一次记录的摘要，不带输出。
    /// 没发过任务的空对话不列。
    pub fn overview(&self) -> StoreResult<Vec<ProjectOverview>> {
        let mut out = Vec::new();
        for workspace in self.workspaces()? {
            let mut tasks = Vec::new();
            for session in self.sessions(&workspace.id)? {
                let run_count: usize = self.conn.query_row(
                    "SELECT COUNT(*) FROM runs WHERE session_id=?1",
                    [&session.id],
                    |row| row.get(0),
                )?;
                if run_count == 0 {
                    continue;
                }
                let last: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT record FROM runs WHERE session_id=?1 ORDER BY number DESC LIMIT 1",
                        [&session.id],
                        |row| row.get(0),
                    )
                    .optional()?;
                let last_run = last.and_then(|raw| serde_json::from_str::<RunBrief>(&raw).ok());
                // 旧版的对话没有自己的名字，用第一句话当任务名。
                let title = if matches!(session.title.as_str(), "新会话" | "默认发令会话" | "新任务")
                {
                    self.conn
                        .query_row(
                            "SELECT content FROM messages WHERE session_id=?1 AND role='user'
                            ORDER BY created_at, rowid LIMIT 1",
                            [&session.id],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?
                        .unwrap_or(session.title.clone())
                } else {
                    session.title.clone()
                };
                tasks.push(TaskBrief {
                    id: session.id,
                    workspace_id: session.workspace_id,
                    title,
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                    run_count,
                    last_run,
                });
            }
            out.push(ProjectOverview { workspace, tasks });
        }
        Ok(out)
    }

    pub fn workspace_for_session(&self, session_id: &str) -> StoreResult<Workspace> {
        let raw: String = self.conn.query_row(
            "SELECT w.data FROM workspaces w
            JOIN sessions s ON s.workspace_id=w.id WHERE s.id=?1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn create_session(
        &self,
        workspace_id: &str,
        title: Option<String>,
    ) -> StoreResult<Session> {
        let now = now_ms();
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.into(),
            title: title
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "新任务".into()),
            created_at: now,
            updated_at: now,
        };
        self.conn.execute(
            "INSERT INTO sessions VALUES(?1, ?2, ?3, ?4, ?5)",
            params![session.id, session.workspace_id, session.title, now, now],
        )?;
        Ok(session)
    }

    pub fn sessions(&self, workspace_id: &str) -> StoreResult<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, title, created_at, updated_at
            FROM sessions WHERE workspace_id=?1 ORDER BY updated_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([workspace_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn messages(&self, session_id: &str) -> StoreResult<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, run_id, created_at
            FROM messages WHERE session_id=?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map([session_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                run_id: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn next_run_number(&self) -> StoreResult<u64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(number), 104) + 1 FROM runs",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn create_run(
        &mut self,
        session_id: &str,
        prompt: &str,
        number: u64,
        plan: &DispatchPlan,
    ) -> StoreResult<RunRecord> {
        let record = RunRecord {
            id: format!("run-{number}"),
            number,
            session_id: session_id.into(),
            flow_id: plan.flow_id.clone(),
            flow_name: plan.flow_name.clone(),
            prompt: prompt.into(),
            status: RunStatus::Running,
            started_at: now_ms(),
            finished_at: None,
            steps: plan
                .steps
                .iter()
                .map(|s| s.to_descriptor(StepStatus::Pending))
                .collect(),
            tasks: vec![],
            files: vec![],
            step_files: Default::default(),
            logs: vec![],
            summary: None,
            who: None,
            permission: None,
            undone_at: None,
        };
        let mut stored = record.clone();
        stored.steps.clear();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO runs(id, number, session_id, record, plan) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                record.id,
                number,
                session_id,
                serde_json::to_string(&stored)?,
                serde_json::to_string(plan)?
            ],
        )?;
        for step in &record.steps {
            tx.execute(
                "INSERT INTO steps VALUES(?1, ?2, ?3, ?4)",
                params![
                    step.id,
                    record.id,
                    step.step_index,
                    serde_json::to_string(step)?
                ],
            )?;
        }
        tx.execute(
            "INSERT INTO messages VALUES(?1, ?2, 'user', ?3, NULL, ?4)",
            params![
                uuid::Uuid::new_v4().to_string(),
                session_id,
                prompt,
                record.started_at
            ],
        )?;
        tx.execute(
            "INSERT INTO messages VALUES(?1, ?2, 'assistant', ?3, ?4, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                session_id,
                format!("交给 {}。改动在右栏。", plan.flow_name),
                record.id,
                record.started_at
            ],
        )?;
        tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![record.started_at, session_id],
        )?;
        tx.commit()?;
        Ok(record)
    }

    pub fn run(&self, run_id: &str) -> StoreResult<RunRecord> {
        let raw: String =
            self.conn
                .query_row("SELECT record FROM runs WHERE id=?1", [run_id], |row| {
                    row.get(0)
                })?;
        let mut record: RunRecord = serde_json::from_str(&raw)?;
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM steps WHERE run_id=?1 ORDER BY step_index")?;
        let rows = stmt
            .query_map([run_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        record.steps = rows
            .into_iter()
            .map(|raw| serde_json::from_str(&raw))
            .collect::<Result<_, _>>()?;
        Ok(record)
    }

    pub fn runs(&self, session_id: &str) -> StoreResult<Vec<RunRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM runs WHERE session_id=?1 ORDER BY number")?;
        let ids = stmt
            .query_map([session_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter().map(|id| self.run(&id)).collect()
    }

    pub fn plan(&self, run_id: &str) -> StoreResult<DispatchPlan> {
        let raw: String =
            self.conn
                .query_row("SELECT plan FROM runs WHERE id=?1", [run_id], |row| {
                    row.get(0)
                })?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn snapshot(&self, run_id: &str) -> StoreResult<Option<RunSnapshotRecord>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT snapshot FROM runs WHERE id=?1", [run_id], |row| {
                row.get(0)
            })
            .optional()?
            .flatten();
        raw.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }

    pub fn save_run(&mut self, record: &RunRecord) -> StoreResult<()> {
        let mut stored = record.clone();
        stored.steps.clear();
        let snapshot = saved_snapshot(&record.id)
            .map(|s| serde_json::to_string(&s))
            .transpose()?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE runs SET record=?1, snapshot=COALESCE(?2, snapshot) WHERE id=?3",
            params![serde_json::to_string(&stored)?, snapshot, record.id],
        )?;
        for step in &record.steps {
            tx.execute(
                "INSERT INTO steps(id,run_id,step_index,data) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                params![step.id, record.id, step.step_index, serde_json::to_string(step)?],
            )?;
        }
        tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![now_ms(), record.session_id],
        )?;
        if let Some(summary) = &record.summary {
            tx.execute(
                "UPDATE messages SET content=?1 WHERE run_id=?2 AND role='assistant'",
                params![summary, record.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_event(&mut self, event: &RunEvent) -> StoreResult<()> {
        let run_id = match event {
            RunEvent::RunStarted { run_id, .. }
            | RunEvent::StepAdded { run_id, .. }
            | RunEvent::StepStarted { run_id, .. }
            | RunEvent::StepExecution { run_id, .. }
            | RunEvent::StepOutput { run_id, .. }
            | RunEvent::TasksUpdated { run_id, .. }
            | RunEvent::FileTouched { run_id, .. }
            | RunEvent::StepFinished { run_id, .. }
            | RunEvent::RunFinished { run_id, .. } => run_id,
        };
        let mut run = self.run(run_id)?;
        let now = now_ms();
        match event {
            RunEvent::StepAdded { step, .. } => run.steps.push(step.clone()),
            RunEvent::RunStarted { steps, .. } => {
                run.steps = steps.clone();
                run.status = RunStatus::Running;
            }
            RunEvent::StepStarted { step_id, .. } => {
                run.status = RunStatus::Running;
                run.finished_at = None;
                run.summary = None;
                if let Some(step) = run.steps.iter_mut().find(|s| s.id == *step_id) {
                    step.status = StepStatus::Running;
                    step.started_at = Some(now);
                    step.finished_at = None;
                    step.exit_code = None;
                }
            }
            RunEvent::StepExecution {
                step_id,
                mode,
                notice,
                ..
            } => {
                if let Some(step) = run.steps.iter_mut().find(|s| s.id == *step_id) {
                    step.execution_mode = Some(mode.clone());
                    step.notice = Some(notice.clone());
                }
            }
            RunEvent::StepOutput {
                step_id,
                stream,
                chunk,
                ..
            } => {
                let agent_id = run
                    .steps
                    .iter()
                    .find(|s| s.id == *step_id)
                    .map(|s| s.agent_id.clone())
                    .unwrap_or_default();
                run.logs.push(RawLogEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    run_id: run.id.clone(),
                    step_id: step_id.clone(),
                    agent_id,
                    stream: *stream,
                    chunk: chunk.clone(),
                    timestamp: now,
                });
            }
            RunEvent::TasksUpdated { items, .. } => run.tasks = items.clone(),
            RunEvent::FileTouched {
                path,
                kind,
                step_id,
                ..
            } => {
                if let Some(file) = run.files.iter_mut().find(|f| f.path == *path) {
                    file.kind = kind.clone();
                    file.step_id = step_id.clone();
                } else {
                    run.files.push(FileChangeItem {
                        path: path.clone(),
                        kind: kind.clone(),
                        step_id: step_id.clone(),
                        additions: 0,
                        deletions: 0,
                    });
                }
            }
            RunEvent::StepFinished {
                step_id,
                status,
                exit_code,
                duration_ms,
                changeset,
                ..
            } => {
                run.step_files
                    .insert(step_id.clone(), changeset.files.clone());
                if let Some(step) = run.steps.iter_mut().find(|s| s.id == *step_id) {
                    step.status = *status;
                    step.exit_code = *exit_code;
                    step.duration_ms = Some(*duration_ms);
                    step.finished_at = Some(now);
                }
                for file in &changeset.files {
                    if let Some(existing) = run.files.iter_mut().find(|f| f.path == file.path) {
                        *existing = file.clone();
                    } else {
                        run.files.push(file.clone());
                    }
                }
            }
            RunEvent::RunFinished {
                status, summary, ..
            } => {
                run.status = *status;
                run.summary = Some(summary.clone());
                run.finished_at = Some(now);
                if *status == RunStatus::Cancelled {
                    for step in &mut run.steps {
                        if matches!(step.status, StepStatus::Running | StepStatus::Pending) {
                            step.status = StepStatus::Cancelled;
                        }
                    }
                }
            }
        }
        self.save_run(&run)
    }

    pub fn recover_interrupted_runs(&mut self) -> StoreResult<()> {
        let mut stmt = self.conn.prepare("SELECT id FROM runs")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for id in ids {
            let mut run = self.run(&id)?;
            if run.status == RunStatus::Running {
                run.status = RunStatus::Failed;
                run.finished_at = Some(now_ms());
                run.summary = Some("上次退出时任务被打断了；可以重跑没做完的那一步。".into());
                for step in &mut run.steps {
                    if step.status == StepStatus::Running {
                        step.status = StepStatus::Failed;
                        step.finished_at = run.finished_at;
                    }
                }
                if !run.steps.iter().any(|s| s.status == StepStatus::Failed) {
                    if let Some(step) = run
                        .steps
                        .iter_mut()
                        .find(|s| s.status == StepStatus::Pending)
                    {
                        step.status = StepStatus::Failed;
                    }
                }
                self.save_run(&run)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::builtin_default_manifests;
    use crate::events::EventSink;
    use crate::flow::{builtin_default_flows, resolve_dispatch_plan};
    use crate::process::ProcessCancelHandle;
    use crate::run::execute_run_from_step;
    use crate::snapshot::{get_diff_for_run, restore_snapshot};
    use crate::workspace::open_workspace;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[tokio::test]
    async fn history_survives_reopen_with_original_diff_and_interrupted_run_recovery() {
        let root = std::env::temp_dir().join(format!("peck-history-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let path = root.join("peck.db");
        let mut db = Store::open(&path).unwrap();
        let ws = open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db
            .create_session(&ws.id, Some("History test".into()))
            .unwrap();
        let agents = builtin_default_manifests();
        let plan = resolve_dispatch_plan(
            "run-700105",
            "@mock build",
            ".peck/handoff/700105",
            workspace.to_str().unwrap(),
            &agents,
            &builtin_default_flows(),
        )
        .unwrap();
        db.create_run(&session.id, "@mock build", 700105, &plan)
            .unwrap();
        let shared = Arc::new(Mutex::new(db));
        let (memory, _) = EventSink::new_memory();
        let sink = EventSink::Persistent {
            target: Box::new(memory),
            store: shared.clone(),
        };
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-700105".into(),
            workspace.clone(),
            ".peck/handoff/700105".into(),
            plan.clone(),
            agents,
            0,
            sink,
            rx,
            Some(Duration::from_millis(45)),
        )
        .await;
        assert_eq!(status, RunStatus::Succeeded);
        let original = shared.lock().unwrap().run("run-700105").unwrap();
        assert_eq!(original.tasks.len(), 3);
        assert!(original.tasks.iter().all(|t| t.completed));
        assert_eq!(original.steps[0].execution_mode.as_deref(), Some("mock"));
        assert!(!original.logs.is_empty());
        let mut unfinished_plan = plan;
        unfinished_plan.steps[0].id = "run-700106-step-1".into();
        shared
            .lock()
            .unwrap()
            .create_run(&session.id, "interrupted", 700106, &unfinished_plan)
            .unwrap();
        shared
            .lock()
            .unwrap()
            .record_event(&RunEvent::StepStarted {
                run_id: "run-700106".into(),
                step_id: "run-700106-step-1".into(),
                step_index: 0,
                agent_id: "mock".into(),
            })
            .unwrap();
        drop(shared);
        std::fs::write(
            workspace.join(".peck/handoff/700105/intent.md"),
            "changed after execution",
        )
        .unwrap();
        let mut reopened = Store::open(&path).unwrap();
        reopened.recover_interrupted_runs().unwrap();
        assert_eq!(reopened.workspaces().unwrap()[0].path, ws.path);
        assert_eq!(reopened.sessions(&ws.id).unwrap()[0].id, session.id);
        assert_eq!(reopened.messages(&session.id).unwrap().len(), 4);
        let loaded = reopened.runs(&session.id).unwrap();
        assert_eq!(loaded[0].status, RunStatus::Succeeded);
        assert_eq!(loaded[1].status, RunStatus::Failed);
        assert_eq!(loaded[1].steps[0].status, StepStatus::Failed);
        assert_eq!(reopened.next_run_number().unwrap(), 700107);
        let mut snapshot = reopened.snapshot("run-700105").unwrap().unwrap();
        snapshot.run_id = format!("restored-{}", uuid::Uuid::new_v4());
        let restored_id = snapshot.run_id.clone();
        restore_snapshot(snapshot);
        let diff = get_diff_for_run(
            &restored_id,
            None,
            Some(".peck/handoff/700105/intent.md"),
            None,
        )
        .unwrap();
        assert!(diff.content_after.unwrap().contains("Mock Intent Spec"));
        assert!(diff.unified_diff.contains("@mock") || diff.unified_diff.contains("build"));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_cli_can_resume_after_reopen_without_rerunning_successful_step() {
        let root = std::env::temp_dir().join(format!("peck-resume-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let path = root.join("peck.db");
        let mut db = Store::open(&path).unwrap();
        let ws = open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, None).unwrap();
        let mut agents = builtin_default_manifests();
        agents[0].command = "/bin/sh".into();
        agents[0].args = vec!["-c".into(), "mkdir -p .peck/handoff/800105; echo '# Intent' > .peck/handoff/800105/intent.md; printf -- '- [ ] Task\\n' > .peck/tasks.md; echo first >> invocations".into()];
        agents[1].command = "/bin/sh".into();
        agents[1].args = vec!["-c".into(), "if [ ! -f permit ]; then echo 'authentication required' >&2; exit 1; fi; printf -- '- [x] Task\\n' > .peck/tasks.md".into()];
        let plan = resolve_dispatch_plan(
            "run-800105",
            "build",
            ".peck/handoff/800105",
            workspace.to_str().unwrap(),
            &agents,
            &builtin_default_flows(),
        )
        .unwrap();
        db.create_run(&session.id, "build", 800105, &plan).unwrap();
        let shared = Arc::new(Mutex::new(db));
        let (memory, _) = EventSink::new_memory();
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-800105".into(),
            workspace.clone(),
            ".peck/handoff/800105".into(),
            plan,
            agents.clone(),
            0,
            EventSink::Persistent {
                target: Box::new(memory),
                store: shared.clone(),
            },
            rx,
            None,
        )
        .await;
        assert_eq!(status, RunStatus::Failed);
        assert_eq!(
            shared.lock().unwrap().run("run-800105").unwrap().steps[1]
                .execution_mode
                .as_deref(),
            Some("cli")
        );
        drop(shared);
        std::fs::write(workspace.join("permit"), "ok").unwrap();
        let reopened = Store::open(&path).unwrap();
        let restored_plan = reopened.plan("run-800105").unwrap();
        restore_snapshot(reopened.snapshot("run-800105").unwrap().unwrap());
        let shared = Arc::new(Mutex::new(reopened));
        let (memory, _) = EventSink::new_memory();
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-800105".into(),
            workspace.clone(),
            ".peck/handoff/800105".into(),
            restored_plan,
            agents,
            1,
            EventSink::Persistent {
                target: Box::new(memory),
                store: shared.clone(),
            },
            rx,
            None,
        )
        .await;
        assert_eq!(status, RunStatus::Succeeded);
        assert_eq!(
            std::fs::read_to_string(workspace.join("invocations"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(shared.lock().unwrap().run("run-800105").unwrap().tasks[0].completed);
        drop(shared);
        std::fs::remove_dir_all(root).unwrap();
    }
}
