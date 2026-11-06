use std::path::PathBuf;
use tauri::{ipc::Channel, State};

use crate::agent::{load_agent_manifests, load_and_probe_all_agents, AgentInfo, AgentManifest};
use crate::events::{EventSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{load_flow_manifests, resolve_dispatch_plan};
use crate::process::ProcessCancelHandle;
use crate::run::{execute_run_from_step, ActiveRunContext};
use crate::snapshot::{get_diff_for_run, restore_snapshot, DiffResult};
use crate::state::AppState;
use crate::store::{Message, RunRecord, Session};
use crate::workspace::{open_workspace, Workspace};

#[tauri::command]
pub async fn workspace_open(path: String, state: State<'_, AppState>) -> Result<Workspace, String> {
    let expanded = crate::agent::expand_tilde(&path);
    let canonical = expanded
        .canonicalize()
        .map_err(|e| format!("无法打开工作区：{e}"))?;
    let ws = open_workspace(&canonical.to_string_lossy())?;
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .save_workspace(&ws)
        .map_err(|e| e.to_string())?;
    Ok(ws)
}

#[tauri::command]
pub async fn workspace_choose(state: State<'_, AppState>) -> Result<Option<Workspace>, String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"选择 Peck 工作区\")",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message.contains("-128") {
            return Ok(None);
        }
        return Err(format!("无法选择文件夹，可改为输入完整路径：{message}"));
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

#[tauri::command]
pub async fn run_dispatch(
    session_id: String,
    prompt: String,
    channel: Channel<RunEvent>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("请输入指令。".into());
    }
    let agents = load_agent_manifests();
    let flows = load_flow_manifests();
    let mut active = state.active_runs.lock().map_err(|e| e.to_string())?;
    let mut db = state.store.lock().map_err(|e| e.to_string())?;
    let ws = db
        .workspace_for_session(&session_id)
        .map_err(|e| e.to_string())?;
    if !PathBuf::from(&ws.path).is_dir() {
        return Err("工作区已不存在，请重新打开文件夹。".into());
    }
    if active
        .values()
        .any(|r| r.workspace_path == PathBuf::from(&ws.path))
    {
        return Err("此工作区已有运行中的任务，请等待完成或先停止。".into());
    }
    let number = db.next_run_number().map_err(|e| e.to_string())?;
    let run_id = format!("run-{number}");
    let handoff_rel = format!(".peck/handoff/{number}");
    let plan = resolve_dispatch_plan(&run_id, &prompt, &handoff_rel, &ws.path, &agents, &flows)?;
    db.create_run(&session_id, &prompt, number, &plan)
        .map_err(|e| e.to_string())?;
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
    if !matches!(record.status, RunStatus::Failed | RunStatus::Cancelled) {
        return Err("只有失败或中止的任务可以重试。".into());
    }
    let plan = db.plan(&run_id).map_err(|e| e.to_string())?;
    let start_index = plan
        .steps
        .iter()
        .position(|s| s.id == step_id)
        .ok_or("未找到需要重试的步骤。")?;
    if record
        .steps
        .iter()
        .take(start_index)
        .any(|s| s.status != StepStatus::Succeeded)
    {
        return Err("请先重试前面未完成的步骤。".into());
    }
    let ws = db
        .workspace_for_session(&record.session_id)
        .map_err(|e| e.to_string())?;
    if !PathBuf::from(&ws.path).is_dir() {
        return Err("工作区已不存在，请重新打开文件夹。".into());
    }
    if active
        .values()
        .any(|r| r.workspace_path == PathBuf::from(&ws.path))
    {
        return Err("此工作区已有运行中的任务。".into());
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
        return Err("无效的 Agent ID".into());
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
        return Err("无效的 Agent ID".into());
    }
    let manifest = AgentManifest::from_toml_str(&content)?;
    if manifest.id != agent_id {
        return Err("清单 id 必须与当前 Agent 一致。".into());
    }
    if manifest.command.trim().is_empty() {
        return Err("command 不能为空。".into());
    }
    if crate::agent::parse_timeout_str(&manifest.timeout).is_none() {
        return Err("timeout 格式无效，例如 30m 或 60s。".into());
    }
    if !matches!(manifest.stdin.as_str(), "none" | "prompt") {
        return Err("stdin 仅支持 none 或 prompt。".into());
    }
    let dir = crate::mode::app_data_dir().join("agents");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{agent_id}.toml")), content).map_err(|e| e.to_string())
}
