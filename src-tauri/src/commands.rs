use std::path::PathBuf;
use tauri::{ipc::Channel, State};

use crate::agent::{load_agent_manifests, load_and_probe_all_agents, AgentInfo, AgentManifest};
use crate::events::{EventSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{load_flow_manifests, resolve_dispatch_plan, resolve_plan_for_who};
use crate::process::ProcessCancelHandle;
use crate::run::{execute_run_from_step, ActiveRunContext};
use crate::snapshot::{
    get_diff_for_run, restore_snapshot, saved_snapshot, DiffResult, ShadowRepo, UndoPreview,
};
use crate::state::AppState;
use crate::store::{now_ms, Message, ProjectOverview, RunRecord, Session};
use crate::workspace::{open_workspace, Workspace};

#[tauri::command]
pub async fn workspace_open(path: String, state: State<'_, AppState>) -> Result<Workspace, String> {
    let expanded = crate::agent::expand_tilde(&path);
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("打不开这个文件夹：{e}"))?;
    let mut ws = open_workspace(&canonical.to_string_lossy())?;
    let db = state.store.lock().map_err(|e| e.to_string())?;
    // 打开过的：留着改过的名字和项目设置；移出过列表的回来。
    if let Ok(old) = db.workspace(&ws.id) {
        ws.name = old.name;
        ws.settings = old.settings;
    }
    db.save_workspace(&ws).map_err(|e| e.to_string())?;
    Ok(ws)
}

/// 项目设置里加一个其他文件夹（v0.7）：系统选文件夹的窗，只返回路径，不打开成项目。
#[tauri::command]
pub async fn dialog_pick_folder() -> Result<Option<String>, String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args(["-e", "POSIX path of (choose folder with prompt \"选一个文件夹加进项目\")"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message.contains("-128") {
            return Ok(None);
        }
        return Err(format!("选不了文件夹：{}", message.trim()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().trim_end_matches('/').to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[tauri::command]
pub async fn workspace_choose(state: State<'_, AppState>) -> Result<Option<Workspace>, String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"选择项目文件夹\")",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message.contains("-128") {
            return Ok(None);
        }
        return Err(format!("选不了文件夹，可以改成输入完整路径：{message}"));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    workspace_open(path, state).await.map(Some)
}

#[tauri::command]
pub async fn workspace_list_recent(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .workspaces()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_create(
    workspace_id: String,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<Session, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .create_session(&workspace_id, title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_list(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Session>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .sessions(&workspace_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_get_messages(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .messages(&session_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_get_runs(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<RunRecord>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .runs(&session_id)
        .map_err(|e| e.to_string())
}

fn spawn_run(
    ctx: ActiveRunContext,
    start_index: usize,
    state: &AppState,
    channel: Channel<RunEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let store = state.store.clone();
    let active_runs = state.active_runs.clone();
    let sink = EventSink::Persistent {
        target: Box::new(EventSink::from_channel(channel)),
        store,
    };
    tokio::spawn(async move {
        execute_run_from_step(
            ctx.run_id.clone(),
            ctx.workspace_path,
            ctx.handoff_rel,
            ctx.plan,
            ctx.agents,
            start_index,
            sink,
            cancel_rx,
            None,
        )
        .await;
        if let Ok(mut runs) = active_runs.lock() {
            runs.remove(&ctx.run_id);
        }
    });
}

fn check_permission(permission: &Option<String>) -> Result<(), String> {
    match permission.as_deref() {
        None | Some("plan") | Some("edit") | Some("auto") => Ok(()),
        Some(_) => Err("权限只有 PLAN、EDIT、AUTO 三档。".into()),
    }
}

/// 发任务。`who` 是「谁来干」（`muse`、`grok`、`muse>grok`、`grok>muse`、`mock`）；
/// 不带 `who` 时照旧：`@名字` 开头走单个，别的走默认的先写规范再实施。
/// `permission` 是这次选的权限档，记在任务上（CLI 的权限参数 v0.5 接）。
#[tauri::command]
pub async fn run_dispatch(
    session_id: String,
    prompt: String,
    who: Option<String>,
    permission: Option<String>,
    channel: Channel<RunEvent>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    check_permission(&permission)?;
    let agents = load_agent_manifests();
    let flows = load_flow_manifests();
    let mut active = state.active_runs.lock().map_err(|e| e.to_string())?;
    let mut db = state.store.lock().map_err(|e| e.to_string())?;
    let ws = db
        .workspace_for_session(&session_id)
        .map_err(|e| e.to_string())?;
    if !PathBuf::from(&ws.path).is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    if active
        .values()
        .any(|r| r.touches(&[PathBuf::from(&ws.path)]))
    {
        return Err("同一个项目一次跑一个任务。".into());
    }
    let number = db.next_run_number().map_err(|e| e.to_string())?;
    let run_id = format!("run-{number}");
    let handoff_rel = format!(".peck/handoff/{number}");
    let plan = match who.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        Some(who) => resolve_plan_for_who(
            &run_id,
            who,
            &prompt,
            &handoff_rel,
            &ws.path,
            &agents,
            &flows,
        )?,
        None => resolve_dispatch_plan(&run_id, &prompt, &handoff_rel, &ws.path, &agents, &flows)?,
    };
    let mut record = db
        .create_run(&session_id, &prompt, number, &plan)
        .map_err(|e| e.to_string())?;
    if who.is_some() || permission.is_some() {
        record.who = who;
        record.permission = permission;
        db.save_run(&record).map_err(|e| e.to_string())?;
    }
    drop(db);
    let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();
    let ctx = ActiveRunContext {
        run_id: run_id.clone(),
        session_id,
        workspace_path: PathBuf::from(ws.path),
        handoff_rel,
        plan,
        agents,
        cancel_handle,
        writable: vec![],
    };
    active.insert(run_id.clone(), ctx.clone());
    drop(active);
    spawn_run(ctx, 0, &state, channel, cancel_rx);
    Ok(run_id)
}

#[tauri::command]
pub async fn run_cancel(run_id: String, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(ctx) = state
        .active_runs
        .lock()
        .map_err(|e| e.to_string())?
        .get(&run_id)
    {
        ctx.cancel_handle.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn run_retry_step(
    run_id: String,
    step_id: String,
    channel: Channel<RunEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let agents = load_agent_manifests();
    let mut active = state.active_runs.lock().map_err(|e| e.to_string())?;
    let mut db = state.store.lock().map_err(|e| e.to_string())?;
    let mut record = db.run(&run_id).map_err(|e| e.to_string())?;
    if record.flow_id.starts_with("api-") {
        return Err("API 模式的任务不重放已经跑过的工具；看一下调用记录，再从现在的文件接着发一个任务。".into());
    }
    // Peck 循环（`peck`）和 CLI 引擎（`cli-*`）没有按步骤的清单，重跑会落到假 Agent 上：接着发一句就行。
    if record.flow_id == "peck" || record.flow_id.starts_with("cli-") {
        return Err("这个任务不按步骤重跑；在下面接着发一句，它会从现在的文件接着做。".into());
    }
    if !matches!(record.status, RunStatus::Failed | RunStatus::Cancelled) {
        return Err("只有失败或已停止的任务能重跑。".into());
    }
    let plan = db.plan(&run_id).map_err(|e| e.to_string())?;
    let start_index = plan
        .steps
        .iter()
        .position(|s| s.id == step_id)
        .ok_or("没找到要重跑的那一步。")?;
    if record
        .steps
        .iter()
        .take(start_index)
        .any(|s| s.status != StepStatus::Succeeded)
    {
        return Err("前面还有没做完的步骤，先重跑前面的。".into());
    }
    let ws = db
        .workspace_for_session(&record.session_id)
        .map_err(|e| e.to_string())?;
    if !PathBuf::from(&ws.path).is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    if active
        .values()
        .any(|r| r.touches(&[PathBuf::from(&ws.path)]))
    {
        return Err("同一个项目一次跑一个任务。".into());
    }
    if let Some(snapshot) = db.snapshot(&run_id).map_err(|e| e.to_string())? {
        restore_snapshot(snapshot);
    }
    for step in record.steps.iter_mut().skip(start_index) {
        step.status = StepStatus::Pending;
        step.exit_code = None;
        step.duration_ms = None;
        step.started_at = None;
        step.finished_at = None;
        step.notice = None;
        step.execution_mode = None;
    }
    record.status = RunStatus::Running;
    record.finished_at = None;
    record.summary = None;
    record.undone_at = None;
    db.save_run(&record).map_err(|e| e.to_string())?;
    drop(db);
    let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();
    let ctx = ActiveRunContext {
        run_id: run_id.clone(),
        session_id: record.session_id,
        workspace_path: PathBuf::from(ws.path),
        handoff_rel: format!(".peck/handoff/{}", record.number),
        plan,
        agents,
        cancel_handle,
        writable: vec![],
    };
    active.insert(run_id, ctx.clone());
    drop(active);
    spawn_run(ctx, start_index, &state, channel, cancel_rx);
    Ok(())
}

#[tauri::command]
pub async fn changes_get_diff(
    run_id: String,
    step_id: Option<String>,
    file_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<DiffResult, String> {
    let db = state.store.lock().map_err(|e| e.to_string())?;
    if let Some(snapshot) = db.snapshot(&run_id).map_err(|e| e.to_string())? {
        restore_snapshot(snapshot);
    }
    drop(db);
    get_diff_for_run(&run_id, step_id.as_deref(), file_path.as_deref(), None)
}

#[tauri::command]
pub async fn agents_list() -> Result<Vec<AgentInfo>, String> {
    Ok(load_and_probe_all_agents().await)
}

#[tauri::command]
pub async fn agent_get_config(agent_id: String) -> Result<String, String> {
    if !agent_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || agent_id.is_empty()
    {
        return Err("这个 CLI 的名字不对。".into());
    }
    let _ = load_agent_manifests();
    std::fs::read_to_string(
        crate::mode::app_data_dir()
            .join("agents")
            .join(format!("{agent_id}.toml")),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_save_config(agent_id: String, content: String) -> Result<(), String> {
    if !agent_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || agent_id.is_empty()
    {
        return Err("这个 CLI 的名字不对。".into());
    }
    let manifest = AgentManifest::from_toml_str(&content)?;
    if manifest.id != agent_id {
        return Err("清单里的 id 要和这个 CLI 一样。".into());
    }
    if manifest.command.trim().is_empty() {
        return Err("command 不能空着。".into());
    }
    if crate::agent::parse_timeout_str(&manifest.timeout).is_none() {
        return Err("timeout 写成 30m 或 60s 这样。".into());
    }
    if !matches!(manifest.stdin.as_str(), "none" | "prompt") {
        return Err("stdin 只能是 none 或 prompt。".into());
    }
    let dir = crate::mode::app_data_dir().join("agents");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{agent_id}.toml")), content).map_err(|e| e.to_string())
}

/// 左栏：项目列表，每个项目带它的任务。
#[tauri::command]
pub async fn projects_overview(state: State<'_, AppState>) -> Result<Vec<ProjectOverview>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .overview()
        .map_err(|e| e.to_string())
}

fn workspace_root(state: &AppState, workspace_id: &str) -> Result<PathBuf, String> {
    let ws = state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .workspace(workspace_id)
        .map_err(|_| "没找到这个项目。".to_string())?;
    let root = PathBuf::from(&ws.path);
    if !root.is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    Ok(root)
}

fn workspace_is_busy(state: &AppState, root: &std::path::Path) -> bool {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    state
        .active_runs
        .lock()
        .map(|runs| {
            runs.values().any(|r| r.touches(&[canonical.clone()]))
        })
        .unwrap_or(false)
}

/// 右栏 FILES：项目里的文件（照忽略规则藏起一部分，写明藏了几个）。
#[tauri::command]
pub async fn files_list(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<crate::files::FileListing, String> {
    let root = workspace_root(&state, &workspace_id)?;
    tokio::task::spawn_blocking(move || crate::files::list_files(&root))
        .await
        .map_err(|e| e.to_string())
}

/// 右栏 FILES：读一个文件给预览。文件已经不在、又给了任务号时，读任务前快照里的样子。
#[tauri::command]
pub async fn files_read(
    workspace_id: String,
    path: String,
    run_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<crate::files::FilePreview, String> {
    let root = workspace_root(&state, &workspace_id)?;
    let mut preview = crate::files::read_preview(&root, &path)?;
    if preview.kind != "missing" {
        return Ok(preview);
    }
    let Some(run_id) = run_id else {
        return Ok(preview);
    };
    let snapshot = match saved_snapshot(&run_id) {
        Some(found) => Some(found),
        None => state
            .store
            .lock()
            .map_err(|e| e.to_string())?
            .snapshot(&run_id)
            .map_err(|e| e.to_string())?,
    };
    if let Some(record) = snapshot {
        let repo = ShadowRepo::with_shadow_dir(&record.workspace_path, &record.shadow_dir)?;
        if let Some(before) = repo.read_file_at_tree(&record.base_tree, &path) {
            preview.size = before.len() as u64;
            preview.kind = "text".into();
            preview.content = Some(before);
            preview.deleted = true;
        }
    }
    Ok(preview)
}

/// FILES：Quick Look 的缩略图（Word、Excel 这类）。只在 Mac 上有。
#[tauri::command]
pub async fn files_quicklook(workspace_id: String, path: String, state: State<'_, AppState>) -> Result<String, String> {
    let root = workspace_root(&state, &workspace_id)?;
    let file = crate::files::inside(&root, &path)?;
    tokio::task::spawn_blocking(move || crate::files::quicklook_thumbnail(&file, 1400)).await.map_err(|e| e.to_string())?
}

/// FILES：叫出系统的 Quick Look 窗口看这个文件。
#[tauri::command]
pub async fn files_quicklook_open(workspace_id: String, path: String, state: State<'_, AppState>) -> Result<(), String> {
    let root = workspace_root(&state, &workspace_id)?;
    crate::files::quicklook_open(&crate::files::inside(&root, &path)?)
}

/// CHANGES：拒绝一块改动（v0.7）。在现在的文件里把这一块换回任务前的样子；后来又改过、对不上就不动。
/// 独立工作区里还没搬回的任务，改的是独立工作区里的文件。
#[tauri::command]
pub async fn changes_hunk_revert(
    run_id: String,
    step_id: Option<String>,
    file_path: String,
    hunk: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (record, snapshot) = {
        let db = state.store.lock().map_err(|e| e.to_string())?;
        let record = db.run(&run_id).map_err(|_| "没找到这个任务。".to_string())?;
        let snapshot = db.snapshot(&run_id).map_err(|e| e.to_string())?.ok_or("这个任务没有任务前快照。")?;
        (record, snapshot)
    };
    if record.status == RunStatus::Running {
        return Err("任务还在跑，跑完再拒绝。".into());
    }
    if record.undone_at.is_some() {
        return Err("这个任务已经撤销了。".into());
    }
    if record.worktree.as_ref().is_some_and(|w| w.discarded_at.is_some()) {
        return Err("独立工作区已经丢掉了。".into());
    }
    if workspace_is_busy(&state, &snapshot.workspace_path) {
        return Err("这个项目有任务在跑，等它做完。".into());
    }
    restore_snapshot(snapshot.clone());
    // 其他文件夹的写绝对路径；项目里的照记录里的文件夹（独立工作区还没搬回时是它）。
    let full = if std::path::Path::new(&file_path).is_absolute() {
        if !snapshot.extras.iter().any(|e| e.rel_of(&file_path).is_some()) {
            return Err("只能改这个任务改过的文件。".into());
        }
        std::path::PathBuf::from(&file_path)
    } else {
        crate::files::check_relative(&file_path)?;
        snapshot.workspace_path.join(&file_path)
    };
    if !record.files.iter().any(|f| f.path == file_path) {
        return Err("这个文件不是这个任务改的。".into());
    }
    let created = record.files.iter().any(|f| f.path == file_path && f.kind == "create");
    let diff = get_diff_for_run(&run_id, step_id.as_deref(), Some(&file_path), None)?;
    tokio::task::spawn_blocking(move || {
        let current = std::fs::read_to_string(&full).ok();
        match crate::snapshot::revert_hunk(current.as_deref(), &diff.unified_diff, hunk, created)? {
            None => std::fs::remove_file(&full).map_err(|e| format!("删不掉：{e}")),
            Some(text) => {
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&full, text).map_err(|e| format!("写不进：{e}"))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn snapshot_for_undo(
    state: &AppState,
    run_id: &str,
) -> Result<(RunRecord, crate::snapshot::RunSnapshotRecord), String> {
    let db = state.store.lock().map_err(|e| e.to_string())?;
    let record = db.run(run_id).map_err(|_| "没找到这个任务。".to_string())?;
    if record.status == RunStatus::Running {
        return Err("任务还在跑，先停下来再撤销。".into());
    }
    if record.undone_at.is_some() {
        return Err("这个任务已经撤销过了。".into());
    }
    if let Some(wt) = &record.worktree {
        if wt.pending() {
            return Err("这个任务在独立工作区里，还没搬回项目：不用撤销，丢掉就行。".into());
        }
        if wt.discarded_at.is_some() {
            return Err("独立工作区已经丢掉了，项目里没有这个任务的改动。".into());
        }
    }
    let snapshot = match saved_snapshot(run_id) {
        Some(found) => found,
        None => db
            .snapshot(run_id)
            .map_err(|e| e.to_string())?
            .ok_or("这个任务没有任务前快照，撤销不了。")?,
    };
    if !snapshot.workspace_path.is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    Ok((record, snapshot))
}

/// 撤销前先看：会动哪些文件，哪些在任务做完以后又变过。
#[tauri::command]
pub async fn run_undo_preview(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<UndoPreview, String> {
    let (_, snapshot) = snapshot_for_undo(&state, &run_id)?;
    tokio::task::spawn_blocking(move || crate::snapshot::undo_preview_all(&snapshot))
        .await
        .map_err(|e| e.to_string())?
}

/// 撤销整个任务：任务改过的文件回到任务前快照；别的文件、用户自己的 Git 都不动。
#[tauri::command]
pub async fn run_undo(run_id: String, state: State<'_, AppState>) -> Result<RunRecord, String> {
    let (mut record, snapshot) = snapshot_for_undo(&state, &run_id)?;
    if workspace_is_busy(&state, &snapshot.workspace_path) {
        return Err("这个项目有任务在跑，等它做完再撤销。".into());
    }
    restore_snapshot(snapshot.clone());
    tokio::task::spawn_blocking(move || crate::snapshot::undo_run(&snapshot))
        .await
        .map_err(|e| e.to_string())??;
    record.undone_at = Some(now_ms());
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .save_run(&record)
        .map_err(|e| e.to_string())?;
    Ok(record)
}

/// 界面设置：一整段 JSON，界面自己定里面有什么。
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .setting("ui")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_set(value: String, state: State<'_, AppState>) -> Result<(), String> {
    if value.len() > 200_000 || serde_json::from_str::<serde_json::Value>(&value).is_err() {
        return Err("设置存不了：内容不对。".into());
    }
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .set_setting("ui", &value)
        .map_err(|e| e.to_string())
}

/// 右栏 TERM：在项目里跑一条命令。模型在跑任务时锁着，不让发。
#[tauri::command]
pub async fn term_run(
    workspace_id: String,
    command: String,
    channel: Channel<crate::term::TermEvent>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let root = workspace_root(&state, &workspace_id)?;
    if workspace_is_busy(&state, &root) {
        return Err("任务在跑，终端先锁着；做完就解开。".into());
    }
    let env = state.store.lock().ok().and_then(|db| db.workspace(&workspace_id).ok()).map(|w| w.settings.env_pairs()).unwrap_or_default();
    state.terminals.run_with_env(&workspace_id, &root, &command, &env, move |event| {
        let _ = channel.send(event);
    })
}

#[tauri::command]
pub async fn term_stop(workspace_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.terminals.stop(&workspace_id);
    Ok(())
}

#[tauri::command]
pub async fn term_state(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<crate::term::TermState, String> {
    let root = workspace_root(&state, &workspace_id)?;
    Ok(state.terminals.state(&workspace_id, &root))
}

/// 菜单栏小图标跟着变：`idle`、`running`、`waiting`。
#[tauri::command]
pub async fn shell_set_activity(activity: String, count: Option<u32>, app: tauri::AppHandle) -> Result<(), String> {
    let activity = crate::tray::Activity::parse(&activity).ok_or("没有这个状态。")?;
    crate::tray::set_activity(&app, activity);
    crate::tray::set_running_count(&app, count.unwrap_or(0));
    Ok(())
}

/// 发一条 macOS 通知（任务做完、要回答）。开关在界面设置里，界面决定发不发。
#[tauri::command]
pub async fn notify(title: String, body: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}
