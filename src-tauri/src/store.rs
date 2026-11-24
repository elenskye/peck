use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::events::{FileChangeItem, RunEvent, RunStatus, StepDescriptor, StepStatus};
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
    /// 归档的时间（v0.9）：归档的不在左栏列，记录全留着。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub number: u64,
    pub session_id: String,
    pub prompt: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub steps: Vec<StepDescriptor>,
    pub files: Vec<FileChangeItem>,
    pub summary: Option<String>,
    /// 谁来干：`api:<供应商>/<模型>`（PLOOP 的主角色）、`cli:<引擎>`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    /// 发任务时选的权限档：`plan` / `edit` / `auto`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
    /// 「撤销整个任务」的时间；没撤销过就没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undone_at: Option<i64>,
    /// 在独立工作区里做的（v0.7）：在哪、搬回去没有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<crate::worktree::WorktreeState>,
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
    /// 归档的时间：只有设置 → ARCHIVE 那份列表里有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<i64>,
    /// 模型提了、站主还没批的 Skill（v0.10 C3）：左栏 NOW 里算「等你」。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skills_waiting: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBrief {
    pub id: String,
    pub number: u64,
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
    /// 项目文件夹不在了（被删、挪走、外接盘没插）。每次读列表都看一遍。
    #[serde(default)]
    pub missing: bool,
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
        let layout = crate::projects::Layout::load(self);
        let mut out = Vec::new();
        let workspaces: Vec<Workspace> = self.workspaces()?.into_iter().filter(|w| !w.hidden).collect();
        for workspace in crate::projects::arrange(workspaces, &layout.projects, |w| &w.id) {
            let mut tasks = Vec::new();
            for session in self.sessions(&workspace.id)? {
                if session.archived_at.is_some() {
                    continue;
                }
                if let Some(task) = self.task_brief(session)? {
                    tasks.push(task);
                }
            }
            let tasks = match layout.tasks.get(&workspace.id) {
                Some(order) => crate::projects::arrange(tasks, order, |t| &t.id),
                None => tasks,
            };
            let missing = crate::projects::folder_missing(&workspace.path);
            out.push(ProjectOverview { workspace, tasks, missing });
        }
        Ok(out)
    }

    /// 归档了的任务，按项目分组（移出列表的项目也算）：最近归档的项目在前，组里最近归档的在前。
    pub fn archived_overview(&self) -> StoreResult<Vec<ProjectOverview>> {
        let mut out: Vec<(i64, ProjectOverview)> = Vec::new();
        for workspace in self.workspaces()? {
            let mut tasks = Vec::new();
            for session in self.sessions(&workspace.id)? {
                if session.archived_at.is_none() {
                    continue;
                }
                if let Some(task) = self.task_brief(session)? {
                    tasks.push(task);
                }
            }
            if tasks.is_empty() {
                continue;
            }
            tasks.sort_by_key(|t| std::cmp::Reverse(t.archived_at.unwrap_or(0)));
            let latest = tasks[0].archived_at.unwrap_or(0);
            let missing = crate::projects::folder_missing(&workspace.path);
            out.push((latest, ProjectOverview { workspace, tasks, missing }));
        }
        out.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        Ok(out.into_iter().map(|(_, p)| p).collect())
    }

    /// 一个任务在列表里的样子；还没发过一句话的（没有记录）不列。
    fn task_brief(&self, session: Session) -> StoreResult<Option<TaskBrief>> {
        let run_count: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE session_id=?1",
            [&session.id],
            |row| row.get(0),
        )?;
        if run_count == 0 {
            return Ok(None);
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
        // 没批的 Skill 提议（v0.10 C3）：没有「存了」「不要」的；「排着」只在那个任务还在跑时算批过
        // （App 中途退出、任务没收尾，排着的没写进去，还要站主再批；v0.10 收尾审查）。没写进去的也还等着。
        let skills_waiting: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM agent_events p JOIN runs r ON r.id = p.run_id
                  WHERE r.session_id = ?1 AND p.type = 'skill.proposed'
                    AND NOT EXISTS (
                      SELECT 1 FROM agent_events q
                       WHERE q.run_id = p.run_id AND q.type = 'skill.reviewed'
                         AND json_extract(q.event, '$.proposalId') = json_extract(p.event, '$.proposalId')
                         AND (json_extract(q.event, '$.status') IN ('saved', 'skipped')
                              OR (json_extract(q.event, '$.status') = 'queued' AND json_extract(r.record, '$.status') = 'running')))",
                [&session.id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let title = session.title.clone();
        Ok(Some(TaskBrief {
            id: session.id,
            workspace_id: session.workspace_id,
            title,
            created_at: session.created_at,
            updated_at: session.updated_at,
            run_count,
            last_run,
            archived_at: session.archived_at,
            skills_waiting: skills_waiting.max(0) as u32,
        }))
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
            archived_at: None,
        };
        self.conn.execute(
            "INSERT INTO sessions(id, workspace_id, title, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![session.id, session.workspace_id, session.title, now, now],
        )?;
        Ok(session)
    }

    pub fn sessions(&self, workspace_id: &str) -> StoreResult<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, title, created_at, updated_at, archived_at
            FROM sessions WHERE workspace_id=?1 ORDER BY updated_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([workspace_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                title: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                archived_at: row.get(5)?,
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
            prompt: prompt.into(),
            status: RunStatus::Running,
            started_at: now_ms(),
            finished_at: None,
            steps: plan
                .steps
                .iter()
                .map(|s| s.to_descriptor(StepStatus::Pending))
                .collect(),
            files: vec![],
            summary: None,
            who: None,
            permission: None,
            undone_at: None,
            worktree: None,
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
        tx.commit()?;
        Ok(())
    }

    pub fn record_event(&mut self, event: &RunEvent) -> StoreResult<()> {
        let run_id = match event {
            RunEvent::RunStarted { run_id, .. }
            | RunEvent::StepStarted { run_id, .. }
            | RunEvent::StepFinished { run_id, .. }
            | RunEvent::RunFinished { run_id, .. } => run_id,
        };
        let mut run = self.run(run_id)?;
        let now = now_ms();
        match event {
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
            RunEvent::StepFinished {
                step_id,
                status,
                exit_code,
                duration_ms,
                changeset,
                ..
            } => {
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
        // 只取还标着在跑的（`RunStatus` 存成小写的 running），不用把每条任务记录都读一遍。
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM runs WHERE json_extract(record, '$.status') = 'running'")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for id in ids {
            let mut run = self.run(&id)?;
            if run.status == RunStatus::Running {
                run.status = RunStatus::Failed;
                run.finished_at = Some(now_ms());
                run.summary = Some("上次退出时任务被打断了。".into());
                for step in &mut run.steps {
                    if step.status == StepStatus::Running {
                        step.status = StepStatus::Failed;
                        step.finished_at = run.finished_at;
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
    use crate::events::{ChangeSetSummary, EventSink};
    use crate::flow::ResolvedStepPlan;
    use crate::snapshot::{get_diff_for_run, record_run_base, record_run_finish, restore_snapshot, ShadowRepo};
    use crate::workspace::open_workspace;
    use std::sync::{Arc, Mutex};

    fn plan_for(run: &str) -> DispatchPlan {
        DispatchPlan {
            steps: vec![ResolvedStepPlan {
                id: format!("{run}-step-1"),
                step_index: 0,
                agent_id: "peck".into(),
                agent_name: "Peck".into(),
                title: "Peck".into(),
            }],
        }
    }

    #[test]
    fn history_survives_reopen_with_original_diff_and_interrupted_run_recovery() {
        let root = std::env::temp_dir().join(format!("peck-history-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("a.txt"), "before\n").unwrap();
        let path = root.join("peck.db");
        let mut db = Store::open(&path).unwrap();
        let ws = open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, Some("History test".into())).unwrap();
        db.create_run(&session.id, "build", 700105, &plan_for("run-700105")).unwrap();
        let shared = Arc::new(Mutex::new(db));
        let sink = EventSink::new(shared.clone());
        // 一个做完的任务：快照、改文件、收尾，都经任务事件存进记录。
        let repo = ShadowRepo::open_or_init(&workspace).unwrap();
        let before = repo.create_snapshot("run-700105-base").unwrap();
        record_run_base("run-700105", &repo, before.clone());
        sink.emit(RunEvent::StepStarted { run_id: "run-700105".into(), step_id: "run-700105-step-1".into(), step_index: 0, agent_id: "peck".into() });
        std::fs::write(workspace.join("a.txt"), "after\n").unwrap();
        let after = repo.create_snapshot("run-700105-step-1-after").unwrap();
        record_run_finish("run-700105", after.clone());
        let changeset = repo.diff_changeset(&before, &after).unwrap();
        assert_eq!(changeset.files.len(), 1);
        sink.emit(RunEvent::StepFinished { run_id: "run-700105".into(), step_id: "run-700105-step-1".into(), status: StepStatus::Succeeded, exit_code: None, duration_ms: 5, changeset });
        sink.emit(RunEvent::RunFinished { run_id: "run-700105".into(), status: RunStatus::Succeeded, summary: "做完了。".into() });
        // 一个跑到一半 App 退出的任务。
        shared.lock().unwrap().create_run(&session.id, "interrupted", 700106, &plan_for("run-700106")).unwrap();
        shared
            .lock()
            .unwrap()
            .record_event(&RunEvent::StepStarted { run_id: "run-700106".into(), step_id: "run-700106-step-1".into(), step_index: 0, agent_id: "peck".into() })
            .unwrap();
        drop(sink);
        drop(shared);
        std::fs::write(workspace.join("a.txt"), "changed later\n").unwrap();
        let mut reopened = Store::open(&path).unwrap();
        reopened.recover_interrupted_runs().unwrap();
        assert_eq!(reopened.workspaces().unwrap()[0].path, ws.path);
        assert_eq!(reopened.sessions(&ws.id).unwrap()[0].id, session.id);
        let loaded = reopened.runs(&session.id).unwrap();
        assert_eq!(loaded[0].status, RunStatus::Succeeded);
        assert_eq!(loaded[0].files.len(), 1);
        assert_eq!(loaded[1].status, RunStatus::Failed);
        assert_eq!(loaded[1].steps[0].status, StepStatus::Failed);
        assert_eq!(reopened.next_run_number().unwrap(), 700107);
        // 历史 Diff 是当时的样子，不是后来改的。
        let mut snapshot = reopened.snapshot("run-700105").unwrap().unwrap();
        snapshot.run_id = format!("restored-{}", uuid::Uuid::new_v4());
        let restored_id = snapshot.run_id.clone();
        restore_snapshot(snapshot);
        let diff = get_diff_for_run(&restored_id, "a.txt").unwrap();
        assert!(diff.contains("+after"), "{diff}");
        let _ = ChangeSetSummary::default();
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
