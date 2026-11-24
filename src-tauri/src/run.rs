//! 在跑的任务的登记（v0.8 删了旧的按清单一步步起 CLI 的派发：接力、假 Agent 都并进了新的发任务）。
use std::path::PathBuf;

use crate::process::ProcessCancelHandle;

/// 在跑的任务（`AppState.active_runs`）：停止开关、在哪个项目、会写哪些文件夹。
#[derive(Clone)]
pub struct ActiveRunContext {
    pub workspace_path: PathBuf,
    pub cancel_handle: ProcessCancelHandle,
    /// 项目文件夹以外这个任务能写的文件夹（项目的「能改」的其他文件夹，v0.7）。
    pub writable: Vec<PathBuf>,
}

impl ActiveRunContext {
    /// 这个任务会写的文件夹和 `folders` 有没有重叠（一样、或者一个在另一个里面）：
    /// 重叠的两个任务会共用一个影子仓库、互相算进对方的改动，不让同时跑。
    pub fn touches(&self, folders: &[PathBuf]) -> bool {
        std::iter::once(&self.workspace_path)
            .chain(self.writable.iter())
            .any(|a| folders.iter().any(|b| overlaps(a, b)))
    }
}

/// 两个文件夹一样，或者一个在另一个里面（比较前先认真实路径）。
pub fn overlaps(a: &std::path::Path, b: &std::path::Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a.starts_with(&b) || b.starts_with(&a)
}


// ───────────────────────── Peck Loop 和 CLI 共用的发任务（v1.1.5 合并） ─────────────────────────

use crate::events::{ChangeSetSummary, EventSink as RunSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{DispatchPlan, ResolvedStepPlan};
use crate::gateway::event::PermissionLevel;
use crate::snapshot::{record_run_base, record_run_finish, ShadowRepo};
use crate::store::{now_ms, Store};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(crate) type ActiveRuns = Arc<Mutex<HashMap<String, ActiveRunContext>>>;

/// 发一个任务要占的东西。
pub(crate) struct ClaimSpec<'a> {
    pub store: &'a Arc<Mutex<Store>>,
    pub active_runs: &'a ActiveRuns,
    pub session_id: &'a str,
    pub prompt: &'a str,
    pub who: &'a str,
    pub permission: Option<String>,
    pub level: PermissionLevel,
    pub worktree: bool,
    /// 任务记录里的引擎：`peck` 或 CLI 引擎 id。
    pub agent_id: &'a str,
    /// 步骤的名字（「Peck · DeepSeek V4 Pro」「Codex」）。
    pub label: &'a str,
}

/// 占好的任务。
pub(crate) struct Claimed {
    pub run_id: String,
    pub step_id: String,
    pub workspace: PathBuf,
    pub workspace_id: String,
    pub settings: crate::workspace::ProjectSettings,
    /// 项目的其他文件夹：（路径，能改）。独立工作区里一律只读。
    pub folders: Vec<(PathBuf, bool)>,
    pub cancel_rx: tokio::sync::watch::Receiver<bool>,
    /// 任务在哪做：项目文件夹，或者新建好的独立工作区。
    pub cwd: PathBuf,
}

/// 查能不能发（项目在、没有没处理完的独立工作区、和在跑的任务不重叠）、占任务号、记任务、登记成在跑，
/// 要独立工作区就建好。占号以后出错的由调用方 [`abandon`]。
pub(crate) fn claim_run(spec: ClaimSpec) -> Result<Claimed, String> {
    if spec.worktree && spec.level == PermissionLevel::Plan {
        return Err("PLAN 只读，不用独立工作区。".into());
    }
    let claimed = {
        let mut active = spec.active_runs.lock().map_err(|e| e.to_string())?;
        let mut db = spec.store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace_for_session(spec.session_id).map_err(|e| e.to_string())?;
        let workspace = PathBuf::from(&ws.path);
        if !workspace.is_dir() {
            return Err("项目文件夹不在了，重新打开一次。".into());
        }
        crate::worktree::pending_in_session(&db, spec.session_id)?;
        // 独立工作区的任务：其他文件夹只读（它们不在独立工作区里，改了就是真改，丢掉时撤不回来）。
        let folders: Vec<(PathBuf, bool)> = if spec.worktree { ws.settings.live_folders().into_iter().map(|(p, _)| (p, false)).collect() } else { ws.settings.live_folders() };
        let writable: Vec<PathBuf> = if spec.level == PermissionLevel::Plan { vec![] } else { folders.iter().filter(|(_, w)| *w).map(|(p, _)| p.clone()).collect() };
        let mut touching = vec![workspace.clone()];
        touching.extend(writable.iter().cloned());
        if active.values().any(|r| r.touches(&touching)) {
            return Err("同一个项目一次跑一个任务（项目的文件夹、能改的其他文件夹和在跑的任务重叠也算）。".into());
        }
        if spec.worktree && !crate::worktree::available(&workspace) {
            return Err("这个项目不是 git 仓库，开不了独立工作区。".into());
        }
        let number = db.next_run_number().map_err(|e| e.to_string())?;
        let run_id = format!("run-{number}");
        let step_id = format!("{run_id}-step-1");
        let plan = DispatchPlan { steps: vec![step_plan(&step_id, spec.agent_id, spec.label)] };
        let mut record = db.create_run(spec.session_id, spec.prompt, number, &plan).map_err(|e| e.to_string())?;
        record.who = Some(spec.who.to_string());
        record.permission = spec.permission.clone();
        if spec.worktree {
            let at = crate::worktree::root_for(&workspace).join(&run_id);
            record.worktree = Some(crate::worktree::WorktreeState { path: at.to_string_lossy().into_owned(), applied_at: None, discarded_at: None });
        }
        db.save_run(&record).map_err(|e| e.to_string())?;
        db.set_run_engine(&run_id, spec.agent_id).map_err(|e| e.to_string())?;
        let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();
        active.insert(run_id.clone(), ActiveRunContext { workspace_path: workspace.clone(), cancel_handle, writable });
        Claimed { run_id, step_id, cwd: workspace.clone(), workspace, workspace_id: ws.id, settings: ws.settings, folders, cancel_rx }
    };
    // 独立工作区：先建好（建不了这个任务就算失败、写明原因）。任务在它里面做，主文件夹不动。
    let cwd = worktree_for(spec.store, spec.active_runs, &claimed.run_id, &claimed.workspace, spec.worktree)?;
    Ok(Claimed { cwd, ..claimed })
}

fn step_plan(step_id: &str, agent_id: &str, label: &str) -> ResolvedStepPlan {
    ResolvedStepPlan { id: step_id.into(), step_index: 0, agent_id: agent_id.into(), agent_name: label.into(), title: label.into() }
}

/// 占了号、还没跑起来就出错了：任务记成失败（写明原因）、不再占着项目。给回原因，好接着 `?`。
pub(crate) fn abandon(store: &Arc<Mutex<Store>>, active_runs: &ActiveRuns, run_id: &str, why: String) -> String {
    if let Ok(mut db) = store.lock() {
        if let Ok(mut record) = db.run(run_id) {
            record.status = RunStatus::Failed;
            record.finished_at = Some(now_ms());
            record.summary = Some(why.clone());
            let _ = db.save_run(&record);
        }
    }
    if let Ok(mut a) = active_runs.lock() {
        a.remove(run_id);
    }
    why
}

/// 独立工作区（在 `cwd` 建好）：建不了就删掉建到一半的、记失败。不要独立工作区就是项目文件夹。
pub(crate) fn worktree_for(store: &Arc<Mutex<Store>>, active_runs: &ActiveRuns, run_id: &str, workspace: &Path, worktree: bool) -> Result<PathBuf, String> {
    if !worktree {
        return Ok(workspace.to_path_buf());
    }
    let at = crate::worktree::root_for(workspace).join(run_id);
    match crate::worktree::create(workspace, &at) {
        Ok(()) => Ok(at.canonicalize().unwrap_or(at)),
        Err(e) => {
            // 建到一半（git 已经登记了）：删掉，不留在站主的仓库里。
            if at.exists() {
                crate::worktree::remove(workspace, &at);
            }
            if let Ok(mut db) = store.lock() {
                if let Ok(mut record) = db.run(run_id) {
                    record.worktree = None;
                    let _ = db.save_run(&record);
                }
            }
            Err(abandon(store, active_runs, run_id, e))
        }
    }
}

/// 一个任务从开始到收尾：前后快照（项目和能改的其他文件夹）、任务事件、在跑的登记。
pub(crate) struct Lifecycle {
    id: String,
    step_id: String,
    run_sink: RunSink,
    repo: Option<ShadowRepo>,
    before: Option<String>,
    extras: Vec<(ShadowRepo, String)>,
    started: Instant,
    active_runs: ActiveRuns,
}

impl Lifecycle {
    /// 拍任务前的快照，发「开始了」。独立工作区的快照也存在项目自己的影子仓库里（改动记录、回退照常）。
    pub fn begin(store: &Arc<Mutex<Store>>, active_runs: &ActiveRuns, c: &Claimed, agent_id: &str, label: &str, level: PermissionLevel, worktree: bool) -> Self {
        let id = c.run_id.clone();
        let repo = if worktree { crate::worktree::shadow_repo(&c.cwd, &c.workspace).ok() } else { ShadowRepo::open_or_init(&c.workspace).ok() };
        let before = repo.as_ref().and_then(|r| {
            let tree = r.create_snapshot(&format!("{id}-base")).ok()?;
            record_run_base(&id, r, tree.clone());
            Some(tree)
        });
        // 能改的其他文件夹（v0.10）也拍快照：改动记在任务上、跟着任务回退。
        let extras = crate::harness::run::snapshot_extras(&id, &c.folders, level);
        let run_sink = RunSink::new(store.clone());
        let step = step_plan(&c.step_id, agent_id, label);
        run_sink.emit(RunEvent::RunStarted { run_id: id.clone(), steps: vec![step.to_descriptor(StepStatus::Pending)] });
        run_sink.emit(RunEvent::StepStarted { run_id: id.clone(), step_id: c.step_id.clone(), step_index: 0, agent_id: agent_id.into() });
        Self { id, step_id: c.step_id.clone(), run_sink, repo, before, extras, started: Instant::now(), active_runs: active_runs.clone() }
    }

    /// 收尾快照：给回这一轮项目里改了的文件（收尾提交用）和整份改动（加上其他文件夹的）。任务还在跑时批的 Skill
    /// （v0.10 C3）这时才写，不算进这次的改动。
    pub fn snapshot_after(&self) -> (Vec<String>, ChangeSetSummary) {
        let mut changeset = ChangeSetSummary::default();
        if let (Some(r), Some(before)) = (self.repo.as_ref(), self.before.as_ref()) {
            if let Ok(after) = r.create_snapshot(&format!("{}-after", self.step_id)) {
                record_run_finish(&self.id, after.clone());
                changeset = r.diff_changeset(before, &after).unwrap_or_default();
            }
        }
        let round_files: Vec<String> = changeset.files.iter().map(|f| f.path.clone()).collect();
        crate::harness::run::finish_extras(&self.id, &self.extras, &mut changeset);
        crate::skill_propose::flush(&self.id);
        (round_files, changeset)
    }

    /// 发「做完了」（步骤和任务），不再占着项目。
    pub fn finish(self, step: StepStatus, run: RunStatus, summary: String, changeset: ChangeSetSummary) {
        self.run_sink.emit(RunEvent::StepFinished { run_id: self.id.clone(), step_id: self.step_id.clone(), status: step, exit_code: None, duration_ms: self.started.elapsed().as_millis() as u64, changeset });
        self.run_sink.emit(RunEvent::RunFinished { run_id: self.id.clone(), status: run, summary });
        if let Ok(mut runs) = self.active_runs.lock() {
            runs.remove(&self.id);
        }
    }
}

/// 按了停（`run_cancel`）：叫一下 `on_cancel`（发打断）。任务收尾时 abort 掉。
pub(crate) fn watch_cancel(mut cancel_rx: tokio::sync::watch::Receiver<bool>, on_cancel: impl FnOnce() + Send + 'static) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while cancel_rx.changed().await.is_ok() {
            if *cancel_rx.borrow() {
                on_cancel();
                break;
            }
        }
    })
}

#[cfg(test)]
mod claim_tests {
    use super::*;

    /// 占号（v1.1.5 合并）：一个项目一次一个；占了号出错就记成失败、放开项目（以前 Peck Loop 那边会一直占着）。
    #[test]
    fn claims_one_run_per_project_and_abandon_frees_it() {
        let root = std::env::temp_dir().join(format!("peck-claim-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        let mut store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace { id: "w1".into(), path: ws_dir.to_string_lossy().into_owned(), name: "试".into(), last_opened_at: 0, git_branch: None, hidden: false, settings: Default::default() };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        store.create_run(&session.id, "占号", 980_001, &DispatchPlan { steps: vec![] }).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: ActiveRuns = Arc::default();
        let spec = || ClaimSpec { store: &store, active_runs: &active, session_id: &session.id, prompt: "改一下", who: "cli:codex", permission: None, level: PermissionLevel::Edit, worktree: false, agent_id: "codex", label: "Codex" };
        let claimed = claim_run(spec()).unwrap();
        assert_eq!(claimed.cwd, claimed.workspace);
        assert!(claim_run(spec()).err().unwrap().contains("一次跑一个"));
        let why = abandon(&store, &active, &claimed.run_id, "读不到设置".into());
        assert_eq!(why, "读不到设置");
        assert!(active.lock().unwrap().is_empty(), "放开了项目");
        let record = store.lock().unwrap().run(&claimed.run_id).unwrap();
        assert_eq!((record.status, record.summary.as_deref()), (RunStatus::Failed, Some("读不到设置")));
        assert!(claim_run(ClaimSpec { worktree: true, level: PermissionLevel::Plan, ..spec() }).err().unwrap().contains("PLAN"));
        let _ = std::fs::remove_dir_all(root);
    }
}
