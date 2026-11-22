use std::path::PathBuf;
use tauri::{ipc::Channel, State};

use crate::snapshot::{
    get_diff_for_run, restore_snapshot, saved_snapshot, DiffResult, ShadowRepo,
};
use crate::state::AppState;
use crate::store::{ProjectOverview, RunRecord, Session};
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
/// 用编辑器或访达打开项目里的文件（v0.8）：`how` 是 `finder`（在访达里选中）、`editor`（`app` 给了就用它，
/// 没给用系统默认的 App）。只认项目里的、项目其他文件夹里的；`app` 只能是一个 App 的名字。
#[tauri::command]
pub async fn open_path(workspace_id: String, path: String, how: String, app: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    let (root, folders) = {
        let db = state.store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace(&workspace_id).map_err(|_| "没找到这个项目。".to_string())?;
        (PathBuf::from(&ws.path), ws.settings.live_folders())
    };
    let target = resolve_open_path(&root, &folders, &path)?;
    let mut cmd = tokio::process::Command::new("/usr/bin/open");
    match how.as_str() {
        "finder" => {
            cmd.arg("-R").arg(&target);
        }
        "editor" => {
            if let Some(app) = app.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) {
                if app.len() > 80 || app.contains('/') || app.starts_with('-') {
                    return Err("编辑器写 App 的名字就行（比如 Visual Studio Code）。".into());
                }
                cmd.arg("-a").arg(app);
            } else if runs_when_opened(&target) {
                // 没填编辑器时系统默认的「打开」会运行它（.app、.command、可执行文件……）：App 在访达里标出来，文件用文本编辑打开。
                cmd.arg(if target.is_dir() { "-R" } else { "-t" });
            }
            cmd.arg(&target);
        }
        _ => return Err("只认 finder、editor。".into()),
    }
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.contains("Unable to find application") { "找不到这个编辑器：在设置 → 外观里换一个，或者空着用系统默认的。".into() } else { format!("打不开：{why}") });
    }
    Ok(())
}

/// 用系统默认的方式「打开」会运行起来的：App 和别的包、脚本类的扩展名、带可执行位的文件。
fn runs_when_opened(p: &std::path::Path) -> bool {
    const RUNS: [&str; 16] = ["app", "bundle", "plugin", "prefpane", "saver", "command", "tool", "terminal", "workflow", "action", "scpt", "scptd", "applescript", "pkg", "mpkg", "fileloc"];
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if RUNS.contains(&ext.as_str()) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if p.is_file() && std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
            return true;
        }
    }
    false
}

/// 要打开的路径：项目里的相对路径，或者项目根、其他文件夹里的绝对路径；`..` 和指到外面的都不行。
pub fn resolve_open_path(root: &std::path::Path, folders: &[(PathBuf, bool)], raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let given = std::path::Path::new(if raw.is_empty() { "." } else { raw });
    let joined = if given.is_absolute() { given.to_path_buf() } else { root.join(given) };
    let real = joined.canonicalize().map_err(|_| "这个文件不在了。".to_string())?;
    let mut allowed = vec![root.canonicalize().unwrap_or_else(|_| root.to_path_buf())];
    allowed.extend(folders.iter().filter_map(|(p, _)| p.canonicalize().ok()));
    if !allowed.iter().any(|a| real.starts_with(a)) {
        return Err("只能打开项目里的文件。".into());
    }
    Ok(real)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_paths_stay_inside_the_project_and_its_folders() {
        let root = std::env::temp_dir().join(format!("peck-open-{}", uuid::Uuid::new_v4()));
        let other = root.with_extension("other");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(root.join("src/a.ts"), "x").unwrap();
        std::fs::write(other.join("b.md"), "y").unwrap();
        assert!(resolve_open_path(&root, &[], "src/a.ts").unwrap().ends_with("src/a.ts"));
        assert!(resolve_open_path(&root, &[], "").is_ok());
        assert!(resolve_open_path(&root, &[], "../x").is_err());
        assert!(resolve_open_path(&root, &[], "/etc/hosts").is_err());
        let b = other.join("b.md").to_string_lossy().into_owned();
        assert!(resolve_open_path(&root, &[], &b).is_err());
        assert!(resolve_open_path(&root, &[(other.clone(), false)], &b).is_ok());
        // 系统默认「打开」会运行的：不直接 open。
        std::fs::write(root.join("run.sh"), "echo").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(root.join("run.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(runs_when_opened(&root.join("run.sh")) && runs_when_opened(std::path::Path::new("x/Tool.app")) && runs_when_opened(std::path::Path::new("go.command")));
        assert!(!runs_when_opened(&root.join("src/a.ts")) && !runs_when_opened(&root.join("src")));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }
}
