//! 定时任务（v0.8 Phase 12）：到点了 Peck 替站主在这个项目里发一句话（一个 API 模型或一个 CLI，
//! Antigravity 只做直接调用、不进来），每次开一个新任务；做完、停下、等你批的都进左栏「待你处理」。
//!
//! - 时间按站主这台 Mac 的时区算：界面存的时候带上和 UTC 差几分钟（`offsetMin`，跟 JS 的
//!   `getTimezoneOffset` 反号），夏令时切换那天可能差一小时，记在 HANDOFF。
//! - App 开着才跑：每 30 秒看一眼到点的。App 关着错过的：打开时在 12 小时以内的补跑一次，更早的跳到下一次。
//! - 到点时这个项目有任务在跑：等它跑完再发（每 30 秒再看），等了 2 小时还在跑就跳过这一次、记一句。

use crate::gateway::event::AgentEvent;
use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

const DAY_MS: i64 = 86_400_000;
/// App 关着错过的：多久以内的补跑一次。
const CATCH_UP_MS: i64 = 12 * 3_600_000;
/// 项目一直在忙：最多等多久。
const WAIT_BUSY_MS: i64 = 2 * 3_600_000;
pub const MAX_PER_PROJECT: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum When {
    /// 每天几点（`HH:MM`）；`weekdays` 为真只在周一到周五。
    Daily { time: String, #[serde(default)] weekdays: bool },
    /// 每隔几小时（1–168）。
    Interval { hours: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(default)]
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub prompt: String,
    /// `api:…` 或 `cli:…`。
    pub who: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// `plan` / `edit` / `auto`。
    pub permission: String,
    pub when: When,
    /// 本机时区和 UTC 差几分钟（东八区是 480）。
    #[serde(default)]
    pub offset_min: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub next_at: Option<i64>,
    #[serde(default)]
    pub last_run_at: Option<i64>,
    /// 上一次开的任务（左栏点过去）。
    #[serde(default)]
    pub last_session: Option<String>,
    /// 上一次怎么样了：发出去了 / 跳过了（为什么）。
    #[serde(default)]
    pub last_note: Option<String>,
    #[serde(default)]
    pub created_at: i64,
}

fn yes() -> bool {
    true
}

fn parse_time(t: &str) -> Option<(i64, i64)> {
    let (h, m) = t.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h, m))
}

impl Schedule {
    /// 规整、校验。
    pub fn checked(mut self) -> Result<Self, String> {
        self.title = self.title.trim().chars().take(60).collect();
        self.prompt = self.prompt.trim().to_string();
        if self.prompt.is_empty() {
            return Err("定时任务要发的那句话还空着。".into());
        }
        if self.title.is_empty() {
            self.title = self.prompt.chars().take(30).collect();
        }
        if !self.who.starts_with("api:") && !self.who.starts_with("cli:") {
            return Err("定时任务选一个 API 模型或一个 CLI。".into());
        }
        if let Some(engine) = self.who.strip_prefix("cli:") {
            if crate::gateway::caps::AgentCaps::for_engine(engine).direct_only {
                return Err("Antigravity 只做直接调用，不进定时任务。".into());
            }
        }
        if !matches!(self.permission.as_str(), "plan" | "edit" | "auto") {
            return Err("权限只有 PLAN、EDIT、AUTO 三档。".into());
        }
        match &self.when {
            When::Daily { time, .. } if parse_time(time).is_none() => return Err(format!("时间写成 09:30 这样：{time}")),
            When::Interval { hours } if !(1..=168).contains(hours) => return Err("每隔几小时：1 到 168。".into()),
            _ => {}
        }
        if !(-14 * 60..=14 * 60).contains(&self.offset_min) {
            return Err("时区不对。".into());
        }
        Ok(self)
    }

    /// `now` 之后的下一次（毫秒）。
    pub fn next_after(&self, now: i64) -> i64 {
        match &self.when {
            When::Interval { hours } => {
                let step = *hours as i64 * 3_600_000;
                match self.last_run_at {
                    Some(last) if last + step > now => last + step,
                    _ => now + step,
                }
            }
            When::Daily { time, weekdays } => {
                let (h, m) = parse_time(time).unwrap_or((9, 0));
                let offset = self.offset_min as i64 * 60_000;
                let local_now = now + offset;
                let local_midnight = local_now.div_euclid(DAY_MS) * DAY_MS;
                let mut at = local_midnight + (h * 60 + m) * 60_000;
                if at <= local_now {
                    at += DAY_MS;
                }
                if *weekdays {
                    // 1970-01-01 是星期四：天数 + 3 对 7 取余，0 是星期一。
                    while (at.div_euclid(DAY_MS) + 3).rem_euclid(7) >= 5 {
                        at += DAY_MS;
                    }
                }
                at - offset
            }
        }
    }
}

impl Store {
    pub fn schedules(&self, workspace_id: Option<&str>) -> StoreResult<Vec<Schedule>> {
        let mut stmt = self.conn.prepare("SELECT record FROM schedules WHERE ?1 IS NULL OR workspace_id = ?1 ORDER BY created_at")?;
        let rows = stmt.query_map(params![workspace_id], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).filter_map(|raw| serde_json::from_str(&raw).ok()).collect())
    }

    pub fn save_schedule(&self, s: &Schedule) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO schedules(id, workspace_id, record, next_at, created_at) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET record = excluded.record, next_at = excluded.next_at",
            params![s.id, s.workspace_id, serde_json::to_string(s)?, if s.enabled { s.next_at } else { None }, s.created_at],
        )?;
        Ok(())
    }

    pub fn delete_schedule(&self, id: &str) -> StoreResult<()> {
        self.conn.execute("DELETE FROM schedules WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 到点了的（开着的、`next_at` ≤ now）。
    pub fn due_schedules(&self, now: i64) -> StoreResult<Vec<Schedule>> {
        let mut stmt = self.conn.prepare("SELECT record FROM schedules WHERE next_at IS NOT NULL AND next_at <= ?1 ORDER BY next_at")?;
        let rows = stmt.query_map(params![now], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).filter_map(|raw| serde_json::from_str(&raw).ok()).collect())
    }
}

/// 给界面的一条事件：哪个项目、哪个任务、哪一次。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fired {
    pub schedule_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub run_id: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledEvent {
    run_id: String,
    workspace_id: String,
    session_id: String,
    prompt: String,
    event: AgentEvent,
}

/// 现在就发一次（到点了、或者站主点了 RUN NOW）：开一个新任务，事件经 `schedule-event` 推给界面。
pub async fn fire(app: &tauri::AppHandle, schedule: &Schedule) -> Result<Fired, String> {
    let state = app.state::<AppState>();
    let (session_id, workspace_id) = {
        let db = state.store.lock().map_err(|_| "读不到项目。")?;
        let ws = db.workspace(&schedule.workspace_id).map_err(|_| "这个项目不在列表里了。")?;
        if !std::path::Path::new(&ws.path).is_dir() {
            return Err("这个项目的文件夹不在了。".into());
        }
        let session = db.create_session(&ws.id, Some(format!("⏰ {}", schedule.title))).map_err(|e| e.to_string())?;
        (session.id, ws.id)
    };
    // 任务号拿到之前的事件先攒着（任务在后台起，可能比任务号先到），拿到再一起推。
    type Slot = std::sync::Arc<std::sync::Mutex<(Option<String>, Vec<AgentEvent>)>>;
    let slot: Slot = Default::default();
    let push = {
        let app = app.clone();
        let (ws, sid, prompt) = (workspace_id.clone(), session_id.clone(), schedule.prompt.clone());
        move |run_id: &str, e: &AgentEvent| {
            let _ = app.emit("schedule-event", ScheduledEvent { run_id: run_id.to_string(), workspace_id: ws.clone(), session_id: sid.clone(), prompt: prompt.clone(), event: e.clone() });
        }
    };
    let out = {
        let (slot, push) = (slot.clone(), push.clone());
        move |e: &AgentEvent| {
            let mut g = slot.lock().unwrap_or_else(|p| p.into_inner());
            match &g.0 {
                Some(id) => push(id, e),
                None => g.1.push(e.clone()),
            }
        }
    };
    // 起不来（没 Key、CLI 不在、项目在忙……）就把刚开的空任务删掉，不然每到点留一个空的 ⏰。
    let started: Result<String, String> = async {
    let level = crate::harness::run::permission_of(Some(&schedule.permission))?;
    let run_id = if let Some(model) = schedule.who.strip_prefix("api:") {
        let provider = crate::gateway::providers::for_model(&state, model).await.map_err(|e| e.message)?;
        crate::harness::run::start_peck_run(
            crate::harness::run::PeckRun {
                store: state.store.clone(),
                active_runs: state.active_runs.clone(),
                session_id: session_id.clone(),
                prompt: schedule.prompt.clone(),
                who: schedule.who.clone(),
                permission: Some(schedule.permission.clone()),
                level,
                model_id: model.to_string(),
                effort: schedule.effort.clone(),
                cost_cap: cost_cap(&state),
                env: crate::harness::tools::bash::shell_env(),
                goal: vec![],
                worktree: false,
                delegate: None,
                images: vec![],
            },
            provider,
            out,
        )?
    } else {
        let engine = schedule.who.trim_start_matches("cli:").to_string();
        let label = crate::harness::history::who_label(Some(&schedule.who));
        let backend = crate::gateway::engines::from_state(&state, &engine).await.map_err(|e| e.message)?;
        crate::gateway::runner::start_engine_run(
            crate::gateway::runner::EngineRun {
                store: state.store.clone(),
                active_runs: state.active_runs.clone(),
                session_id: session_id.clone(),
                prompt: schedule.prompt.clone(),
                who: schedule.who.clone(),
                permission: Some(schedule.permission.clone()),
                level,
                engine,
                label,
                model: schedule.model.clone(),
                effort: schedule.effort.clone(),
                resume: None,
                default_account: true,
                worktree: false,
                attachments: vec![],
                delegate: None,
                cost_cap: None,
                goal: vec![],
            },
            backend,
            out,
        )?
    };
    Ok(run_id)
    }
    .await;
    let run_id = match started {
        Ok(id) => id,
        Err(e) => {
            if let Ok(mut db) = state.store.lock() {
                let _ = db.delete_session(&session_id);
            }
            return Err(e);
        }
    };
    {
        let mut g = slot.lock().unwrap_or_else(|p| p.into_inner());
        g.0 = Some(run_id.clone());
        for e in std::mem::take(&mut g.1) {
            push(&run_id, &e);
        }
    }
    let fired = Fired { schedule_id: schedule.id.clone(), workspace_id, session_id, run_id, prompt: schedule.prompt.clone() };
    let _ = app.emit("schedule-fired", fired.clone());
    Ok(fired)
}

/// 界面设置里的每月花费上限（定时任务的 Peck 循环也受它管）。
fn cost_cap(state: &AppState) -> Option<f64> {
    let raw = state.store.lock().ok()?.setting("ui").ok()??;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?.get("costCap")?.as_f64()
}

/// 这个项目现在有任务在跑。
fn busy(state: &AppState, workspace_id: &str) -> bool {
    let Ok(db) = state.store.lock() else { return true };
    let Ok(ws) = db.workspace(workspace_id) else { return false };
    let path = std::path::PathBuf::from(&ws.path);
    state.active_runs.lock().map(|a| a.values().any(|r| r.touches(&[path.clone()]))).unwrap_or(true)
}

/// 打开 App 时起：每 30 秒看一眼到点的。
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 打开 App 时：关着的时候错过的，12 小时以内的补一次，更早的跳到下一次。
        {
            let state = app.state::<AppState>();
            let now = now_ms();
            let stale: Vec<Schedule> = state.store.lock().ok().and_then(|db| db.due_schedules(now - CATCH_UP_MS).ok()).unwrap_or_default();
            for mut s in stale {
                s.last_note = Some("App 关着，错过了一次".into());
                s.next_at = Some(s.next_after(now));
                if let Ok(db) = state.store.lock() {
                    let _ = db.save_schedule(&s);
                }
            }
        }
        loop {
            tick(&app).await;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

/// 发完（或跳过）以后记一笔：重新读库里现在的那一份再改，发的时候站主删了就不写回来、改了就不盖掉。
/// `old_next` 是读出来时的下一次：库里还是它才往后排（站主中途改了、关了，用站主的）。
fn record(state: &AppState, id: &str, old_next: Option<i64>, now: i64, note: String, session: Option<String>, ran: bool) {
    let Ok(db) = state.store.lock() else { return };
    let Some(mut cur) = db.schedules(None).ok().and_then(|l| l.into_iter().find(|x| x.id == id)) else { return };
    if ran {
        cur.last_run_at = Some(now);
        cur.last_session = session;
    }
    cur.last_note = Some(note);
    if old_next.is_some() && cur.next_at == old_next {
        cur.next_at = Some(cur.next_after(now));
    }
    let _ = db.save_schedule(&cur);
}

async fn tick(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let now = now_ms();
    let due: Vec<Schedule> = state.store.lock().ok().and_then(|db| db.due_schedules(now).ok()).unwrap_or_default();
    for s in due {
        if busy(&state, &s.workspace_id) {
            if now - s.next_at.unwrap_or(now) > WAIT_BUSY_MS {
                record(&state, &s.id, s.next_at, now, "项目一直有任务在跑，跳过了一次".into(), None, false);
            }
            continue;
        }
        match fire(app, &s).await {
            Ok(f) => record(&state, &s.id, s.next_at, now, "发出去了".into(), Some(f.session_id), true),
            Err(e) => record(&state, &s.id, s.next_at, now, format!("没发出去：{e}"), None, false),
        }
    }
}

#[tauri::command]
pub async fn schedule_list(workspace_id: Option<String>, state: tauri::State<'_, AppState>) -> Result<Vec<Schedule>, String> {
    let db = state.store.lock().map_err(|_| "读不到定时任务。")?;
    db.schedules(workspace_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn schedule_save(schedule: Schedule, state: tauri::State<'_, AppState>) -> Result<Schedule, String> {
    let mut s = schedule.checked()?;
    let db = state.store.lock().map_err(|_| "存不了定时任务。")?;
    db.workspace(&s.workspace_id).map_err(|_| "没找到这个项目。")?;
    let existing = db.schedules(Some(&s.workspace_id)).map_err(|e| e.to_string())?;
    match existing.iter().find(|x| x.id == s.id && !s.id.is_empty()) {
        Some(old) => {
            s.created_at = old.created_at;
            s.last_run_at = old.last_run_at;
            s.last_session = old.last_session.clone();
            s.last_note = old.last_note.clone();
        }
        None => {
            if existing.len() >= MAX_PER_PROJECT {
                return Err(format!("一个项目最多 {MAX_PER_PROJECT} 个定时任务。"));
            }
            s.id = format!("sched-{}", uuid::Uuid::new_v4().simple());
            s.created_at = now_ms();
        }
    }
    s.next_at = s.enabled.then(|| s.next_after(now_ms()));
    db.save_schedule(&s).map_err(|e| e.to_string())?;
    Ok(s)
}

#[tauri::command]
pub async fn schedule_delete(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let db = state.store.lock().map_err(|_| "删不了定时任务。")?;
    db.delete_schedule(&id).map_err(|e| e.to_string())
}

/// RUN NOW：不等到点，现在就发一次（不改下一次的时间）。
#[tauri::command]
pub async fn schedule_run_now(id: String, app: tauri::AppHandle) -> Result<Fired, String> {
    let s = {
        let state = app.state::<AppState>();
        let db = state.store.lock().map_err(|_| "读不到定时任务。")?;
        db.schedules(None).map_err(|e| e.to_string())?.into_iter().find(|s| s.id == id).ok_or("没找到这个定时任务。")?
    };
    {
        let state = app.state::<AppState>();
        if busy(&state, &s.workspace_id) {
            return Err("这个项目有任务在跑，跑完再发。".into());
        }
    }
    let fired = fire(&app, &s).await?;
    // RUN NOW 不改下一次的时间（old_next 传 None）。
    record(&app.state::<AppState>(), &s.id, None, now_ms(), "手动发了一次".into(), Some(fired.session_id.clone()), true);
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daily(time: &str, weekdays: bool, offset_min: i32) -> Schedule {
        Schedule {
            id: "s".into(),
            workspace_id: "w".into(),
            title: "早报".into(),
            prompt: "看一下昨天的提交".into(),
            who: "api:deepseek/deepseek-flash".into(),
            model: None,
            effort: None,
            permission: "plan".into(),
            when: When::Daily { time: time.into(), weekdays },
            offset_min,
            enabled: true,
            next_at: None,
            last_run_at: None,
            last_session: None,
            last_note: None,
            created_at: 0,
        }
    }

    #[test]
    fn next_times_follow_the_local_clock() {
        // 2026-11-13（星期五）00:30 UTC = 东八区 08:30。
        let now = 1_794_530_200_000 - (1_794_530_200_000 % DAY_MS) + 30 * 60_000;
        let s = daily("09:00", false, 480);
        // 当天东八区 09:00 = 01:00 UTC。
        assert_eq!(s.next_after(now), now + 30 * 60_000);
        // 过了就是明天。
        assert_eq!(s.next_after(now + 31 * 60_000), now + 30 * 60_000 + DAY_MS);
        // 只在工作日：星期五过了 9 点，下一次是星期一。
        let weekday = (now.div_euclid(DAY_MS) + 3).rem_euclid(7);
        let s = daily("09:00", true, 480);
        let next = s.next_after(now + 31 * 60_000);
        let next_day = ((next + 480 * 60_000).div_euclid(DAY_MS) + 3).rem_euclid(7);
        assert!(next_day < 5, "落在周末了：{next_day}");
        if weekday == 4 {
            assert_eq!(next, now + 30 * 60_000 + 3 * DAY_MS);
        }
        // 每隔几小时：从上一次算；错过了从现在算。
        let mut s = daily("09:00", false, 0);
        s.when = When::Interval { hours: 6 };
        assert_eq!(s.next_after(now), now + 6 * 3_600_000);
        s.last_run_at = Some(now - 3_600_000);
        assert_eq!(s.next_after(now), now + 5 * 3_600_000);
    }

    #[test]
    fn checks_reject_bad_input_and_antigravity() {
        assert!(daily("9:00", false, 480).checked().is_ok());
        assert!(daily("25:00", false, 480).checked().is_err());
        let mut s = daily("09:00", false, 480);
        s.who = "cli:antigravity".into();
        assert!(s.checked().unwrap_err().contains("Antigravity"));
        let mut s = daily("09:00", false, 480);
        s.prompt = "  ".into();
        assert!(s.checked().is_err());
        let mut s = daily("09:00", false, 480);
        s.when = When::Interval { hours: 0 };
        assert!(s.checked().is_err());
    }

    #[test]
    fn store_keeps_due_ones() {
        let path = std::env::temp_dir().join(format!("peck-sched-{}.db", uuid::Uuid::new_v4()));
        let store = Store::open(&path).unwrap();
        let mut a = daily("09:00", false, 480);
        a.next_at = Some(100);
        store.save_schedule(&a).unwrap();
        let mut b = daily("10:00", false, 480);
        b.id = "b".into();
        b.next_at = Some(500);
        store.save_schedule(&b).unwrap();
        let mut off = daily("10:00", false, 480);
        off.id = "off".into();
        off.enabled = false;
        off.next_at = Some(50);
        store.save_schedule(&off).unwrap();
        let due = store.due_schedules(200).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "s");
        assert_eq!(store.schedules(Some("w")).unwrap().len(), 3);
        store.delete_schedule("b").unwrap();
        assert_eq!(store.schedules(None).unwrap().len(), 2);
    }
}
