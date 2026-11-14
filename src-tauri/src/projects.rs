//! 左栏的项目和任务：重命名、项目设置、移出列表、文件夹不在了重新定位、在访达里显示、删任务、拖动排的顺序。
//!
//! - 项目「移出列表」只是藏起来（`Workspace.hidden`）：文件夹、任务、快照都不动，重新打开这个文件夹就回来。
//! - 任务「删除」是真的删：任务记录、消息、事件日志、Peck 循环的对话一起删（项目里的文件不动，快照里的历史 Diff 也不再列）。
//! - 项目 id 是文件夹路径的哈希，快照目录也按它放：重新定位到新文件夹时，任务、「总是允许」、快照都挪到新 id 下。
//! - 顺序存在 `app_settings` 的 `layout` 键：拖过的按拖的排，没拖过的（新项目、新任务）排在最上面。

use crate::state::AppState;
use crate::store::{Store, StoreResult};
use crate::workspace::{ProjectSettings, Workspace};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tauri::State;

const LAYOUT: &str = "layout";

/// 拖动排过的顺序。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Layout {
    pub projects: Vec<String>,
    /// 项目 id → 任务 id 的顺序。
    pub tasks: HashMap<String, Vec<String>>,
}

impl Layout {
    pub fn load(store: &Store) -> Self {
        store
            .setting(LAYOUT)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self, store: &Store) -> StoreResult<()> {
        store.set_setting(LAYOUT, &serde_json::to_string(self)?)
    }
}

/// 按拖过的顺序排：不在顺序里的（新的）保持原来的先后排在前面，排过的照排过的顺序跟在后面。
pub fn arrange<T>(items: Vec<T>, order: &[String], id: impl Fn(&T) -> &str) -> Vec<T> {
    let rank: HashMap<&str, usize> = order.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let (mut fresh, mut ranked): (Vec<T>, Vec<T>) = items.into_iter().partition(|t| !rank.contains_key(id(t)));
    ranked.sort_by_key(|t| rank[id(t)]);
    fresh.append(&mut ranked);
    fresh
}

impl Store {
    fn session_running(&self, session_id: &str) -> StoreResult<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE session_id=?1 AND json_extract(record, '$.status') = 'running'",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    fn workspace_running(&self, workspace_id: &str) -> StoreResult<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM runs r JOIN sessions s ON s.id = r.session_id
             WHERE s.workspace_id=?1 AND json_extract(r.record, '$.status') = 'running'",
            [workspace_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE sessions SET title=?2, updated_at=updated_at WHERE id=?1",
            params![session_id, title],
        )?;
        Ok(())
    }

    /// 删一个任务和它的全部记录。
    pub fn delete_session(&mut self, session_id: &str) -> StoreResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM agent_events WHERE run_id IN (SELECT id FROM runs WHERE session_id=?1)",
            [session_id],
        )?;
        tx.execute(
            "DELETE FROM steps WHERE run_id IN (SELECT id FROM runs WHERE session_id=?1)",
            [session_id],
        )?;
        // 旧 API 层（规划 + 实施、假 Agent 走过的）的调用记录指着任务：断开就行，花费记录留着照算。
        tx.execute(
            "UPDATE api_tasks SET run_id=NULL WHERE run_id IN (SELECT id FROM runs WHERE session_id=?1)",
            [session_id],
        )?;
        tx.execute("DELETE FROM messages WHERE session_id=?1", [session_id])?;
        tx.execute("DELETE FROM runs WHERE session_id=?1", [session_id])?;
        tx.execute("DELETE FROM harness_conversations WHERE session_id=?1", [session_id])?;
        tx.execute("DELETE FROM sessions WHERE id=?1", [session_id])?;
        tx.commit()?;
        Ok(())
    }

    /// 项目搬到新文件夹（新 id）：任务、「总是允许」挪过去，旧的那条删掉；新 id 已经有记录就合进去。
    pub fn move_workspace(&mut self, from: &Workspace, to: &Workspace) -> StoreResult<()> {
        let tx = self.conn.transaction()?;
        // 先有新项目，任务才能指过去（sessions 有外键）。
        tx.execute(
            "INSERT INTO workspaces(id, data, last_opened_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data, last_opened_at=excluded.last_opened_at",
            params![to.id, serde_json::to_string(to)?, to.last_opened_at],
        )?;
        tx.execute("UPDATE sessions SET workspace_id=?2 WHERE workspace_id=?1", params![from.id, to.id])?;
        tx.execute(
            "UPDATE OR IGNORE harness_allow SET workspace_id=?2 WHERE workspace_id=?1",
            params![from.id, to.id],
        )?;
        tx.execute("DELETE FROM harness_allow WHERE workspace_id=?1", [&from.id])?;
        tx.execute("UPDATE memories SET workspace_id=?2 WHERE workspace_id=?1", params![from.id, to.id])?;
        tx.execute("DELETE FROM workspaces WHERE id=?1", [&from.id])?;
        tx.commit()?;
        Ok(())
    }
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Store>, String> {
    state.store.lock().map_err(|_| "读不到项目记录。".to_string())
}

/// 改项目的名字、设置（默认谁来干、默认权限）。
#[tauri::command]
pub fn workspace_update(
    id: String,
    name: Option<String>,
    settings: Option<ProjectSettings>,
    state: State<'_, AppState>,
) -> Result<Workspace, String> {
    let db = lock(&state)?;
    let mut ws = db.workspace(&id).map_err(|_| "没找到这个项目。")?;
    if let Some(name) = name {
        let name = name.trim();
        if name.is_empty() {
            return Err("名字不能空着。".into());
        }
        ws.name = name.chars().take(80).collect();
    }
    if let Some(mut settings) = settings {
        // 验证命令的两样只由 verify_save / verify_dismiss 改；界面改别的设置时手里那份可能是旧的。
        settings.verify_dismissed = ws.settings.verify_dismissed;
        settings.verify_approved = ws.settings.verify_approved.clone();
        ws.settings = settings.normalized(Path::new(&ws.path))?;
    }
    db.save_workspace(&ws).map_err(|e| e.to_string())?;
    Ok(ws)
}

/// 移出列表：只是藏起来，文件夹、任务、快照都不动；重新打开这个文件夹就回来。
#[tauri::command]
pub fn workspace_remove(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = lock(&state)?;
    if db.workspace_running(&id).map_err(|e| e.to_string())? {
        return Err("这个项目有任务在跑，先停下再移出。".into());
    }
    let mut ws = db.workspace(&id).map_err(|_| "没找到这个项目。")?;
    ws.hidden = true;
    db.save_workspace(&ws).map_err(|e| e.to_string())?;
    let mut layout = Layout::load(&db);
    layout.projects.retain(|p| p != &id);
    layout.save(&db).map_err(|e| e.to_string())
}

/// 文件夹不在了（或者挪了地方）：换成新的文件夹。`path` 空着就弹选文件夹的窗。
#[tauri::command]
pub async fn workspace_relocate(
    id: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<Workspace>, String> {
    let path = match path {
        Some(p) => p,
        None => match choose_folder().await? {
            Some(p) => p,
            None => return Ok(None),
        },
    };
    let canonical = crate::agent::expand_tilde(&path)
        .canonicalize()
        .map_err(|e| format!("打不开这个文件夹：{e}"))?;
    let mut fresh = crate::workspace::open_workspace(&canonical.to_string_lossy())?;
    let mut db = lock(&state)?;
    let old = db.workspace(&id).map_err(|_| "没找到这个项目。")?;
    if db.workspace_running(&id).map_err(|e| e.to_string())? {
        return Err("这个项目有任务在跑，先停下再换文件夹。".into());
    }
    fresh.name = old.name.clone();
    fresh.settings = old.settings.clone();
    if fresh.id != old.id {
        db.move_workspace(&old, &fresh).map_err(|e| e.to_string())?;
        // 快照按文件夹路径的哈希放：挪到新路径的哈希下，历史 Diff 才读得到。
        move_dir(
            &crate::snapshot::resolve_shadow_dir(Path::new(&old.path)),
            &crate::snapshot::resolve_shadow_dir(&canonical),
        );
        let mut layout = Layout::load(&db);
        for p in layout.projects.iter_mut().filter(|p| **p == old.id) {
            *p = fresh.id.clone();
        }
        if let Some(tasks) = layout.tasks.remove(&old.id) {
            layout.tasks.insert(fresh.id.clone(), tasks);
        }
        layout.save(&db).map_err(|e| e.to_string())?;
    } else {
        db.save_workspace(&fresh).map_err(|e| e.to_string())?;
    }
    Ok(Some(fresh))
}

/// 新位置还没有快照（`resolve_shadow_dir` 刚建了个空的）才挪；已经有了就留着两份，不合并。
fn move_dir(from: &Path, to: &Path) {
    if from == to || !from.exists() {
        return;
    }
    let empty = std::fs::read_dir(to).map(|mut d| d.next().is_none()).unwrap_or(true);
    if empty {
        let _ = std::fs::remove_dir(to);
        let _ = std::fs::rename(from, to);
    }
}

async fn choose_folder() -> Result<Option<String>, String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args(["-e", "POSIX path of (choose folder with prompt \"选择项目文件夹\")"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message.contains("-128") {
            return Ok(None);
        }
        return Err(format!("选不了文件夹：{message}"));
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

/// 在访达里显示项目文件夹。
#[tauri::command]
pub async fn workspace_reveal(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let path = lock(&state)?.workspace(&id).map_err(|_| "没找到这个项目。")?.path;
    if !Path::new(&path).exists() {
        return Err("这个项目的文件夹不在了。".into());
    }
    tokio::process::Command::new("/usr/bin/open")
        .arg(&path)
        .status()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn session_rename(id: String, title: String, state: State<'_, AppState>) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("名字不能空着。".into());
    }
    let title: String = title.chars().take(120).collect();
    lock(&state)?.rename_session(&id, &title).map_err(|e| e.to_string())
}

/// 删一个任务：在跑的不能删。
#[tauri::command]
pub fn session_delete(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut db = lock(&state)?;
    if db.session_running(&id).map_err(|e| e.to_string())? {
        return Err("这个任务在跑，先停下再删。".into());
    }
    db.delete_session(&id).map_err(|e| e.to_string())
}

/// 存拖动排好的顺序：项目的，或者某个项目里任务的。
#[tauri::command]
pub fn layout_save(
    projects: Option<Vec<String>>,
    workspace_id: Option<String>,
    tasks: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = lock(&state)?;
    let mut layout = Layout::load(&db);
    if let Some(projects) = projects {
        layout.projects = projects;
    }
    if let (Some(ws), Some(tasks)) = (workspace_id, tasks) {
        layout.tasks.insert(ws, tasks);
    }
    layout.save(&db).map_err(|e| e.to_string())
}

/// 文件夹在不在（左栏用）。
pub fn folder_missing(path: &str) -> bool {
    !Path::new(path).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::now_ms;

    fn temp_store() -> (std::path::PathBuf, Store) {
        let root = std::env::temp_dir().join(format!("peck-projects-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        (root, store)
    }

    fn ws(id: &str, path: &str) -> Workspace {
        Workspace {
            id: id.into(),
            path: path.into(),
            name: id.into(),
            last_opened_at: now_ms(),
            git_branch: None,
            gitignore_configured: true,
            hidden: false,
            settings: ProjectSettings::default(),
        }
    }

    #[test]
    fn arrange_puts_new_items_first_then_the_dragged_order() {
        let items = vec!["a", "b", "c", "d"];
        let order = vec!["c".to_string(), "a".to_string()];
        assert_eq!(arrange(items, &order, |s| s), vec!["b", "d", "c", "a"]);
    }

    #[test]
    fn tasks_rename_delete_and_projects_move() {
        let (root, mut store) = temp_store();
        let old = ws("ws-old", "/nowhere/old");
        store.save_workspace(&old).unwrap();
        let s = store.create_session("ws-old", Some("第一句".into())).unwrap();
        store.conn.execute(
            "INSERT INTO messages VALUES('m1', ?1, 'user', 'hi', NULL, 1)",
            [&s.id],
        ).unwrap();
        store.harness_allow("ws-old", "install-deps").unwrap();
        store.rename_session(&s.id, "改过的名字").unwrap();
        assert_eq!(store.sessions("ws-old").unwrap()[0].title, "改过的名字");

        // 搬家：任务和「总是允许」跟过去，旧项目没了
        let fresh = ws("ws-new", "/somewhere/new");
        store.move_workspace(&old, &fresh).unwrap();
        assert_eq!(store.sessions("ws-new").unwrap().len(), 1);
        assert!(store.workspace("ws-old").is_err());
        assert!(store.harness_allowed("ws-new").unwrap().contains("install-deps"));

        // 旧 API 层的记录指着这个任务（外键）：删任务不能因为它失败，调用记录留着。
        store.conn.execute(
            "INSERT INTO runs(id, number, session_id, record, plan) VALUES('run-x', 9001, ?1, '{}', '{}')",
            [&s.id],
        ).unwrap();
        store.conn.execute("INSERT INTO api_tasks(id, run_id, role, status) VALUES('t1', 'run-x', 'worker', 'done')", []).unwrap();
        store.conn.execute("INSERT INTO api_calls(id, task_id, started_at, record) VALUES('c1', 't1', 1, '{}')", []).unwrap();

        store.delete_session(&s.id).unwrap();
        assert!(store.sessions("ws-new").unwrap().is_empty());
        let kept: i64 = store.conn.query_row("SELECT COUNT(*) FROM api_calls", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1);
        assert!(store.messages(&s.id).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
