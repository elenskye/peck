//! 提示词库（v1.2.0 Phase 3）：全局一份、所有项目通用，右栏 PROMPT 里增删改查。
//!
//! 一条：标题、正文、建的时间、改的时间。正文照原样存（`{占位}` 不解析，插进输入框时才用）；
//! 列表最近改的在上（新加的算改过）。

use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

/// 标题最多这么多个字；正文最多这么多字节。
pub const TITLE_MAX: usize = 120;
pub const BODY_MAX: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Store {
    /// 全部提示词，最近改的在上。
    pub fn prompts(&self) -> StoreResult<Vec<Prompt>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, created_at, updated_at FROM prompts ORDER BY updated_at DESC, created_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Prompt { id: r.get(0)?, title: r.get(1)?, body: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 改的时间：现在，但一定比库里最新的那条晚（同一毫秒里连着改，刚改的照样排最上）。
    fn prompt_stamp(&self) -> StoreResult<i64> {
        let latest: Option<i64> = self.conn.query_row("SELECT MAX(updated_at) FROM prompts", [], |r| r.get(0)).optional()?.flatten();
        Ok(latest.map_or(now_ms(), |at| now_ms().max(at + 1)))
    }

    /// 存一条新的（标题、正文先用 `clean` 规整过）。
    pub fn add_prompt(&self, title: &str, body: &str) -> StoreResult<Prompt> {
        let at = self.prompt_stamp()?;
        let prompt = Prompt { id: uuid::Uuid::new_v4().to_string(), title: title.into(), body: body.into(), created_at: at, updated_at: at };
        self.conn.execute(
            "INSERT INTO prompts(id, title, body, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![prompt.id, prompt.title, prompt.body, prompt.created_at, prompt.updated_at],
        )?;
        Ok(prompt)
    }

    /// 改标题和正文，刷新改的时间；没有这一条返回 false。
    pub fn update_prompt(&self, id: &str, title: &str, body: &str) -> StoreResult<bool> {
        let at = self.prompt_stamp()?;
        Ok(self.conn.execute("UPDATE prompts SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1", params![id, title, body, at])? > 0)
    }

    /// 删一条；没有这一条返回 false。
    pub fn delete_prompt(&self, id: &str) -> StoreResult<bool> {
        Ok(self.conn.execute("DELETE FROM prompts WHERE id = ?1", params![id])? > 0)
    }
}

/// 存之前规整：标题并成一行；正文去掉开头的空行和末尾的空白，中间照原样（换行、缩进、`{占位}`）。
pub fn clean(title: &str, body: &str) -> Result<(String, String), String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut body = body.trim_end();
    while let Some(i) = body.find('\n') {
        if !body[..i].trim().is_empty() {
            break;
        }
        body = &body[i + 1..];
    }
    if title.is_empty() || body.trim().is_empty() {
        return Err("标题和正文都要写。".into());
    }
    if title.chars().count() > TITLE_MAX {
        return Err(format!("标题最多 {TITLE_MAX} 个字。"));
    }
    if body.len() > BODY_MAX {
        return Err(format!("正文最多 {} KB。", BODY_MAX / 1024));
    }
    Ok((title, body.to_string()))
}

pub(crate) fn list(db: &Store) -> Result<Vec<Prompt>, String> {
    db.prompts().map_err(|e| e.to_string())
}

pub(crate) fn add(db: &Store, title: &str, body: &str) -> Result<Vec<Prompt>, String> {
    let (title, body) = clean(title, body)?;
    db.add_prompt(&title, &body).map_err(|e| e.to_string())?;
    list(db)
}

pub(crate) fn update(db: &Store, id: &str, title: &str, body: &str) -> Result<Vec<Prompt>, String> {
    let (title, body) = clean(title, body)?;
    if !db.update_prompt(id, &title, &body).map_err(|e| e.to_string())? {
        return Err("这一条不在了。".into());
    }
    list(db)
}

pub(crate) fn delete(db: &Store, id: &str) -> Result<Vec<Prompt>, String> {
    db.delete_prompt(id).map_err(|e| e.to_string())?;
    list(db)
}

// ───────────────────────── IPC ─────────────────────────

const LOCKED: &str = "读不到提示词。";

#[tauri::command]
pub fn prompt_list(state: tauri::State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    list(&*state.store.lock().map_err(|_| LOCKED)?)
}

/// 新加一条；返回全部（最近改的在上）。
#[tauri::command]
pub fn prompt_add(title: String, body: String, state: tauri::State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    add(&*state.store.lock().map_err(|_| LOCKED)?, &title, &body)
}

#[tauri::command]
pub fn prompt_update(id: String, title: String, body: String, state: tauri::State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    update(&*state.store.lock().map_err(|_| LOCKED)?, &id, &title, &body)
}

#[tauri::command]
pub fn prompt_delete(id: String, state: tauri::State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    delete(&*state.store.lock().map_err(|_| LOCKED)?, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (std::path::PathBuf, Store) {
        let root = std::env::temp_dir().join(format!("peck-prompts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        (root, store)
    }

    #[test]
    fn add_edit_delete_and_order_by_last_change() {
        let (root, store) = temp_store();
        assert!(list(&store).unwrap().is_empty());
        let after_a = add(&store, "代码审查", "看一下这次的改动。").unwrap();
        assert_eq!(after_a.len(), 1);
        let a = after_a[0].clone();
        assert_eq!(a.created_at, a.updated_at);
        let b = add(&store, "解释一个文件", "读 {文件名}，说清楚它做什么。\n不用改代码。").unwrap()[0].clone();
        assert_eq!(b.title, "解释一个文件");
        assert_eq!(b.body, "读 {文件名}，说清楚它做什么。\n不用改代码。", "占位照原样存");
        assert_eq!(list(&store).unwrap().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [b.id.as_str(), a.id.as_str()], "新加的在上");

        // 改了的排到最上，建的时间不变、改的时间更新。
        let after = update(&store, &a.id, "  代码\n审查 ", "看一下这次的改动：\n- 有没有 Bug").unwrap();
        assert_eq!(after[0].id, a.id);
        assert_eq!(after[0].title, "代码 审查", "标题并成一行");
        assert_eq!(after[0].body, "看一下这次的改动：\n- 有没有 Bug");
        assert_eq!(after[0].created_at, a.created_at);
        assert!(after[0].updated_at > b.updated_at);
        assert!(update(&store, "nope", "x", "y").unwrap_err().contains("不在"));

        // 删。
        let left = delete(&store, &a.id).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, b.id);
        assert!(!store.delete_prompt(&a.id).unwrap());
        assert!(delete(&store, &b.id).unwrap().is_empty());

        // 重开还在（全局一份，不分项目）。
        add(&store, "补测试", "给 {函数名} 补测试").unwrap();
        drop(store);
        let reopened = Store::open(&root.join("peck.db")).unwrap();
        assert_eq!(list(&reopened).unwrap()[0].body, "给 {函数名} 补测试");
        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_keeps_the_body_and_rejects_empty_or_huge() {
        assert_eq!(
            clean(" 标题 ", "\n  \n  - 第一行\n\n    缩进的 {占位}  \n\n").unwrap(),
            ("标题".to_string(), "  - 第一行\n\n    缩进的 {占位}".to_string())
        );
        assert!(clean("", "正文").is_err());
        assert!(clean("标题", " \n\t\n").is_err());
        assert!(clean(&"字".repeat(TITLE_MAX + 1), "正文").unwrap_err().contains("标题"));
        assert!(clean(&"字".repeat(TITLE_MAX), "正文").is_ok());
        assert!(clean("标题", &"a".repeat(BODY_MAX + 1)).unwrap_err().contains("正文"));
    }
}
