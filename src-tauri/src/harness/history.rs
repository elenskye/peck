//! 记忆和历史任务（v0.6 Phase 7）。
//!
//! - **记忆**：模型用 `memory` 工具提「想记住的」，任务结束时中栏一张卡片列出来，站主勾了 SAVE 才存进
//!   `memories`（按项目）；每次发任务放进系统提示的项目层。项目设置里能看、能删。
//! - **@ 旧任务**：站主在输入框里 `@` 一个旧任务（插进去的是 `[@标题](peck:task/<任务 id>)`，也认手写的
//!   `@任务 12`），发出去时 Peck 把那个任务问了什么、做成什么样、改了哪些文件放进这一句话前面。
//! - **搜历史任务**：`search_tasks` 工具按关键词搜这个项目的旧任务，或者按任务号看一次的详情。

use crate::events::RunStatus;
use crate::gateway::event::{AgentEvent, EventBody};
use crate::gateway::log::EventSink;
use crate::state::AppState;
use crate::store::{now_ms, RunRecord, Store, StoreResult};
use rusqlite::params;
use serde::Serialize;
use std::sync::{Arc, Mutex};

/// 一条记忆最长这么多字节；一次最多提这么多条。
pub const NOTE_MAX: usize = 400;
pub const NOTES_PER_CALL: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryNote {
    pub id: String,
    pub text: String,
    /// 哪个任务（run id）提的；手工加的为空。
    pub source_run: Option<String>,
    pub created_at: i64,
}

impl Store {
    pub fn memories(&self, workspace_id: &str) -> StoreResult<Vec<MemoryNote>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, text, source_run, created_at FROM memories WHERE workspace_id = ?1 ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map([workspace_id], |r| {
            Ok(MemoryNote { id: r.get(0)?, text: r.get(1)?, source_run: r.get(2)?, created_at: r.get(3)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 存一条；同一个项目里一字不差的已经有了就不再存（返回 false）。
    pub fn add_memory(&self, workspace_id: &str, text: &str, source_run: Option<&str>) -> StoreResult<bool> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(false);
        }
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE workspace_id = ?1 AND text = ?2)",
            params![workspace_id, text],
            |r| r.get(0),
        )?;
        if exists {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT INTO memories(id, workspace_id, text, source_run, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![uuid::Uuid::new_v4().to_string(), workspace_id, text, source_run, now_ms()],
        )?;
        Ok(true)
    }

    pub fn delete_memory(&self, workspace_id: &str, id: &str) -> StoreResult<()> {
        self.conn.execute("DELETE FROM memories WHERE workspace_id = ?1 AND id = ?2", params![workspace_id, id])?;
        Ok(())
    }

    /// 任务号 → 这个任务（只认这个项目里的）。
    pub fn run_by_number(&self, workspace_id: &str, number: u64) -> StoreResult<Option<RunRecord>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT r.id FROM runs r JOIN sessions s ON s.id = r.session_id WHERE r.number = ?1 AND s.workspace_id = ?2",
                params![number as i64, workspace_id],
                |r| r.get(0),
            )
            .ok();
        id.map(|id| self.run(&id)).transpose()
    }
}

/// 一条记忆规整一下：去掉列表记号、压成一行、截断。空的为 None。
pub fn clean_note(raw: &str) -> Option<String> {
    let one = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = one.trim_start_matches(['-', '*', '•', ' ']).trim();
    if one.is_empty() {
        return None;
    }
    Some(super::tools::clip_bytes(one, NOTE_MAX).to_string())
}

// ───────────────────────── 旧任务 ─────────────────────────

fn status_word(r: &RunRecord) -> &'static str {
    if r.undone_at.is_some() {
        return "已撤销";
    }
    match r.status {
        RunStatus::Running => "在跑",
        RunStatus::Succeeded => "完成",
        RunStatus::Failed => "失败",
        RunStatus::Cancelled => "已停止",
    }
}

fn day(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ms / 1000)
        .map(|t| t.date().to_string())
        .unwrap_or_default()
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.len() <= max {
        flat
    } else {
        format!("{}…", super::tools::clip_bytes(&flat, max))
    }
}

/// 一次任务的详情（给模型看）。
fn run_detail(r: &RunRecord, title: &str) -> String {
    let mut out = format!(
        "Task {} ({}, {}, in \"{}\")\nAsked: {}\n",
        r.number,
        day(r.started_at),
        status_word(r),
        title,
        super::tools::clip_bytes(r.prompt.trim(), 1200)
    );
    if let Some(s) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("Result: {}\n", super::tools::clip_bytes(s.trim(), 2000)));
    }
    if !r.files.is_empty() {
        let files: Vec<String> = r.files.iter().take(30).map(|f| format!("{} (+{} −{})", f.path, f.additions, f.deletions)).collect();
        out.push_str(&format!("Files changed: {}\n", files.join(", ")));
    }
    out
}

/// 输入框里 `@` 进来的旧任务：`peck:task/<任务 id>` 和手写的 `@任务 12` / `@task 12`。
pub fn mentions(prompt: &str) -> (Vec<String>, Vec<u64>) {
    let mut sessions = Vec::new();
    let mut rest = prompt;
    while let Some(i) = rest.find("peck:task/") {
        let tail = &rest[i + "peck:task/".len()..];
        let id: String = tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if !id.is_empty() && !sessions.contains(&id) {
            sessions.push(id.clone());
        }
        rest = &tail[id.len()..];
    }
    let mut numbers = Vec::new();
    for marker in ["@任务", "@task", "@Task"] {
        let mut rest = prompt;
        while let Some(i) = rest.find(marker) {
            let tail = rest[i + marker.len()..].trim_start();
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() {
                if !numbers.contains(&n) {
                    numbers.push(n);
                }
            }
            rest = &rest[i + marker.len()..];
        }
    }
    (sessions, numbers)
}

/// 发出去时放进这一句话前面的引用：每个旧任务一段 `<referenced-task>`。只认这个项目里的；最多 4 个。
pub fn task_refs(store: &Store, workspace_id: &str, prompt: &str) -> Vec<String> {
    let (sessions, numbers) = mentions(prompt);
    let mut out = Vec::new();
    for sid in sessions.iter().take(4) {
        let Ok(ws) = store.workspace_for_session(sid) else { continue };
        if ws.id != workspace_id {
            continue;
        }
        let title = store
            .sessions(workspace_id)
            .ok()
            .and_then(|s| s.into_iter().find(|x| &x.id == sid))
            .map(|s| s.title)
            .unwrap_or_default();
        let runs = store.runs(sid).unwrap_or_default();
        let skip = runs.len().saturating_sub(5);
        let body: Vec<String> = runs.iter().skip(skip).map(|r| run_detail(r, &title)).collect();
        if body.is_empty() {
            continue;
        }
        out.push(format!(
            "{} id=\"{sid}\" title=\"{}\">\nThe user referenced this earlier task. What was asked and what came of it:\n{}</referenced-task>",
            super::context::REF_TAG,
            title.replace('"', "'"),
            super::tools::clip_bytes(&body.join("\n"), 6000)
        ));
    }
    for n in numbers.iter().take(4usize.saturating_sub(out.len())) {
        let Ok(Some(r)) = store.run_by_number(workspace_id, *n) else { continue };
        let title = store.sessions(workspace_id).ok().and_then(|s| s.into_iter().find(|x| x.id == r.session_id)).map(|s| s.title).unwrap_or_default();
        out.push(format!(
            "{} number=\"{n}\">\nThe user referenced task {n}:\n{}</referenced-task>",
            super::context::REF_TAG,
            run_detail(&r, &title)
        ));
    }
    out
}

/// `search_tasks` 工具：有任务号看那一次的详情；有关键词按词搜（每个词都要出现）；都没有列最近 10 次。
pub fn search_tasks(store: &Store, workspace_id: &str, query: Option<&str>, number: Option<u64>) -> Result<String, String> {
    if let Some(n) = number {
        let r = store
            .run_by_number(workspace_id, n)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("这个项目里没有任务 {n}。"))?;
        let title = store.sessions(workspace_id).ok().and_then(|s| s.into_iter().find(|x| x.id == r.session_id)).map(|s| s.title).unwrap_or_default();
        return Ok(run_detail(&r, &title));
    }
    let words: Vec<String> = query.unwrap_or("").split_whitespace().map(|w| w.to_lowercase()).collect();
    let mut hits: Vec<(i64, String)> = Vec::new();
    for s in store.sessions(workspace_id).map_err(|e| e.to_string())? {
        for r in store.runs(&s.id).unwrap_or_default() {
            let hay = format!("{} {} {} {}", s.title, r.prompt, r.summary.clone().unwrap_or_default(), r.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join(" ")).to_lowercase();
            if words.iter().all(|w| hay.contains(w.as_str())) {
                let result = r.summary.as_deref().map(|x| format!(" → {}", one_line(x, 160))).unwrap_or_default();
                hits.push((
                    r.started_at,
                    format!("- Task {} ({}, {}, \"{}\"): {}{}", r.number, day(r.started_at), status_word(&r), one_line(&s.title, 40), one_line(&r.prompt, 160), result),
                ));
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    let total = hits.len();
    if total == 0 {
        return Ok(if words.is_empty() { "这个项目还没有别的任务。".into() } else { format!("没找到和「{}」有关的任务。", words.join(" ")) });
    }
    let shown: Vec<String> = hits.into_iter().take(10).map(|(_, l)| l).collect();
    let head = if words.is_empty() { "最近的任务：".to_string() } else { format!("找到 {total} 个（最新的在前）：") };
    Ok(format!("{head}\n{}\n用 number 看某一次的详情。", shown.join("\n")))
}

/// 给 Peck 循环的 `search_tasks`：这个项目的历史（闭包里握着数据库和项目 id）。
pub fn probe(store: Arc<Mutex<Store>>, workspace_id: String) -> super::r#loop::HistoryProbe {
    Arc::new(move |query: Option<String>, number: Option<u64>| {
        let db = store.lock().map_err(|_| "读不到任务记录。".to_string())?;
        search_tasks(&db, &workspace_id, query.as_deref(), number)
    })
}

// ───────────────────────── IPC ─────────────────────────

#[tauri::command]
pub fn memory_list(workspace_id: String, state: tauri::State<'_, AppState>) -> Result<Vec<MemoryNote>, String> {
    let db = state.store.lock().map_err(|_| "读不到记忆。")?;
    db.memories(&workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn memory_delete(workspace_id: String, id: String, state: tauri::State<'_, AppState>) -> Result<Vec<MemoryNote>, String> {
    let db = state.store.lock().map_err(|_| "读不到记忆。")?;
    db.delete_memory(&workspace_id, &id).map_err(|e| e.to_string())?;
    db.memories(&workspace_id).map_err(|e| e.to_string())
}

/// 站主在「想记住这几条」卡片上批：`keep` 存进这个项目的记忆，`skip` 不要。事件日志里追加一条
/// `memory.reviewed`（卡片据此收起，重开 App 也一样），返回这条事件。
#[tauri::command]
pub fn memory_review(run_id: String, keep: Vec<String>, skip: Vec<String>, state: tauri::State<'_, AppState>) -> Result<AgentEvent, String> {
    review(&state.store, &run_id, keep, skip)
}

pub(crate) fn review(store: &Arc<Mutex<Store>>, run_id: &str, keep: Vec<String>, skip: Vec<String>) -> Result<AgentEvent, String> {
    let (engine, saved) = {
        let db = store.lock().map_err(|_| "读不到任务记录。")?;
        let events = db.agent_events(run_id).map_err(|e| e.to_string())?;
        if events.iter().any(|e| matches!(e.body, EventBody::MemoryReviewed { .. })) {
            return Err("这几条已经批过了。".into());
        }
        if !events.iter().any(|e| matches!(e.body, EventBody::MemoryProposed { .. })) {
            return Err("这个任务没提过要记住什么。".into());
        }
        let record = db.run(run_id).map_err(|_| "没找到这个任务。")?;
        if record.status == RunStatus::Running {
            return Err("任务还在跑，跑完再批。".into());
        }
        let ws = db.workspace_for_session(&record.session_id).map_err(|e| e.to_string())?;
        let mut saved = Vec::new();
        // 站主在卡片上改过字的照存：批的是站主。
        for note in keep.iter().filter_map(|n| clean_note(n)) {
            if db.add_memory(&ws.id, &note, Some(run_id)).map_err(|e| e.to_string())? {
                saved.push(note);
            }
        }
        (db.run_engine(run_id).ok().flatten().unwrap_or_else(|| "peck".into()), saved)
    };
    let sink = EventSink::new(run_id.to_string(), None, engine, Some(store.clone()), |_| {});
    let skipped = skip.iter().filter_map(|n| clean_note(n)).collect();
    Ok(sink.emit(EventBody::MemoryReviewed { saved, skipped }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::DispatchPlan;

    fn temp_store() -> (std::path::PathBuf, Store, crate::workspace::Workspace) {
        let root = std::env::temp_dir().join(format!("peck-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("ws")).unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: root.join("ws").to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            gitignore_configured: false,
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        (root, store, ws)
    }

    fn add_run(store: &mut Store, session: &str, number: u64, prompt: &str, summary: &str) -> String {
        let plan = DispatchPlan { flow_id: "peck".into(), flow_name: "Peck".into(), clean_input: prompt.into(), steps: vec![] };
        let mut r = store.create_run(session, prompt, number, &plan).unwrap();
        r.status = RunStatus::Succeeded;
        r.summary = Some(summary.into());
        store.save_run(&r).unwrap();
        r.id
    }

    #[test]
    fn memories_are_per_project_and_deduplicated() {
        let (root, store, ws) = temp_store();
        assert!(store.add_memory(&ws.id, "测试用 pnpm test", Some("run-1")).unwrap());
        assert!(!store.add_memory(&ws.id, "  测试用 pnpm test ", None).unwrap());
        assert!(store.add_memory("w2", "测试用 pnpm test", None).unwrap());
        let list = store.memories(&ws.id).unwrap();
        assert_eq!(list.len(), 1);
        store.delete_memory(&ws.id, &list[0].id).unwrap();
        assert!(store.memories(&ws.id).unwrap().is_empty());
        assert_eq!(clean_note(" - 用  pnpm\n跑测试 "), Some("用 pnpm 跑测试".into()));
        assert_eq!(clean_note("  "), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mentions_find_linked_tasks_and_task_numbers() {
        let (s, n) = mentions("接着 [@登录页](peck:task/abc-1) 做，参考 @任务 12 和 @task12，还有 @任务12");
        assert_eq!(s, vec!["abc-1"]);
        assert_eq!(n, vec![12]);
        assert_eq!(mentions("没有引用"), (vec![], vec![]));
    }

    #[test]
    fn refs_and_search_stay_inside_the_project() {
        let (root, mut store, ws) = temp_store();
        let s1 = store.create_session(&ws.id, Some("修登录页".into())).unwrap();
        add_run(&mut store, &s1.id, 501, "登录按钮点了没反应", "改了 src/Login.tsx 的 onClick");
        add_run(&mut store, &s1.id, 502, "再加个记住我", "加好了");
        let other = crate::workspace::Workspace { id: "w2".into(), path: "/x".into(), ..ws.clone() };
        store.save_workspace(&other).unwrap();
        let s2 = store.create_session("w2", Some("别的项目".into())).unwrap();
        add_run(&mut store, &s2.id, 503, "登录", "别处的");

        let refs = task_refs(&store, &ws.id, &format!("接着 [@修登录页](peck:task/{}) 做，参考 @任务 503", s1.id));
        assert_eq!(refs.len(), 1, "别的项目的任务不带");
        assert!(refs[0].starts_with("<referenced-task") && refs[0].contains("onClick") && refs[0].contains("记住我"));
        assert_eq!(task_refs(&store, &ws.id, &format!("看 @任务 501 和 peck:task/{}", s2.id)).len(), 1);

        let found = search_tasks(&store, &ws.id, Some("登录 onclick"), None).unwrap();
        assert!(found.contains("Task 501") && !found.contains("503"), "{found}");
        assert!(search_tasks(&store, &ws.id, None, None).unwrap().contains("Task 502"));
        assert!(search_tasks(&store, &ws.id, None, Some(502)).unwrap().contains("记住我"));
        assert!(search_tasks(&store, &ws.id, None, Some(503)).is_err());
        assert!(search_tasks(&store, &ws.id, Some("没有这个"), None).unwrap().contains("没找到"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reviewing_saves_the_kept_notes_once_and_logs_it() {
        let (root, mut store, ws) = temp_store();
        let s = store.create_session(&ws.id, None).unwrap();
        let run = add_run(&mut store, &s.id, 601, "x", "y");
        let store = Arc::new(Mutex::new(store));
        EventSink::new(run.clone(), None, "peck", Some(store.clone()), |_| {})
            .emit(EventBody::MemoryProposed { notes: vec!["测试用 pnpm test".into(), "别动 vendor/".into()] });
        let e = review(&store, &run, vec!["测试用 pnpm test".into()], vec!["别动 vendor/".into()]).unwrap();
        assert!(matches!(&e.body, EventBody::MemoryReviewed { saved, skipped } if saved.len() == 1 && skipped.len() == 1));
        assert_eq!(e.seq, 1, "接着这个任务的事件编号");
        assert_eq!(store.lock().unwrap().memories(&ws.id).unwrap()[0].source_run.as_deref(), Some(run.as_str()));
        assert!(review(&store, &run, vec!["再来".into()], vec![]).unwrap_err().contains("批过"));
        let _ = std::fs::remove_dir_all(root);
    }
}
