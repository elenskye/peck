//! 定时任务：到点了 Peck 替站主在这个项目里发一句话。入口在对话里（v1.0.2）：模型用 `schedule_task` /
//! `list_schedules` / `cancel_schedule` 设、看、取消（`harness/extras.rs`，Peck Loop 直接调、CLI 经本机 MCP 调），
//! 界面只在 PROJECTS 头上列出来看。
//!
//! - **谁来干**：设的时候 AGENTS 里选的那一套（接力、审查、委派都带着）；站主话里点了名的就用点名的那一个。
//!   Antigravity 只做直接调用、不进来。默认 AUTO，高危照样等站主批。
//! - **在哪跑**：默认每次开一个新任务（标题前 ⏰）；站主说了「接在这个对话里」的记着那个对话，到点在里面接着发
//!   （那个对话不在了、归档了就开新的）。
//! - **时间**按这台 Mac 现在的时区算（每次算下一次时现读，夏令时跟着走）。App 开着才跑：每 30 秒看一眼到点的。
//! - **错过的**（App 关着；或者项目一直有任务在跑、等了 2 小时）不自己补跑：记一笔 `missed_at`，界面在那个项目里
//!   弹一张卡片（RUN NOW / SKIP）。重复的任务错过多次也只记最近的那一次。只跑一次的任务跑完、跳过就删掉。

use crate::gateway::event::AgentEvent;
use crate::harness::team::TeamPick;
use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

const DAY_MS: i64 = 86_400_000;
/// 项目一直在忙：最多等多久，过了算错过。
const WAIT_BUSY_MS: i64 = 2 * 3_600_000;
pub const MAX_PER_PROJECT: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum When {
    /// 只跑一次：哪天（`YYYY-MM-DD`）几点（`HH:MM`）。
    Once { date: String, time: String },
    /// 每天几点（`HH:MM`）；`days` 写了就只在那几天（1 是周一 … 7 是周日）；`weekdays` 是旧记录里的「周一到周五」。
    Daily {
        time: String,
        #[serde(default)]
        weekdays: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        days: Vec<u8>,
    },
    /// 每月几号（1–31，那个月没有这一天就用最后一天）几点。
    Monthly { day: u8, time: String },
    /// 每隔几小时（1–168）。
    Interval { hours: u32 },
}

/// 谁来干的那一套：实施的（`api:…` / `cli:…`）、CLI 里的模型、思考强度，加上接力、审查、委派。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    pub who: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub plan_by: Option<TeamPick>,
    #[serde(default)]
    pub review_by: Option<TeamPick>,
    #[serde(default)]
    pub delegate: Option<String>,
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
    /// 接力里写规范的、审查的、委派给谁（设的时候 AGENTS 里选了才有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_by: Option<TeamPick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_by: Option<TeamPick>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegate: Option<String>,
    /// `plan` / `edit` / `auto`。
    pub permission: String,
    pub when: When,
    /// 上次算时间用的时区（和 UTC 差几分钟，东八区是 480）；只是记一下，每次算都现读。
    #[serde(default)]
    pub offset_min: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub next_at: Option<i64>,
    #[serde(default)]
    pub last_run_at: Option<i64>,
    /// 上一次在哪个任务里跑的（界面点过去）。
    #[serde(default)]
    pub last_session: Option<String>,
    /// 上一次怎么样了：发出去了 / 错过了（为什么）。
    #[serde(default)]
    pub last_note: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    /// 接在哪个对话里跑（站主说了才有）；空着是每次开新任务。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// 错过的那一次原本该几点跑（等站主在卡片上点 RUN NOW / SKIP）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed_at: Option<i64>,
}

fn yes() -> bool {
    true
}

fn parse_time(t: &str) -> Option<(i64, i64)> {
    let (h, m) = t.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some((h, m))
}

fn parse_date(d: &str) -> Option<(i64, i64, i64)> {
    let mut it = d.trim().split('-');
    let (y, m, day): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    (it.next().is_none() && (1970..=9999).contains(&y) && (1..=12).contains(&m) && day >= 1 && day <= days_in_month(y, m)).then_some((y, m, day))
}

/// 公历日期 → 从 1970-01-01 起第几天。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 从 1970-01-01 起第几天 → 公历（年、月、日）。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)
}

/// 这台 Mac 现在的时区和 UTC 差几分钟（问系统的 `date +%z`，十分钟读一次；读不到用上次的，再没有就是 0）。
pub fn local_offset_min() -> i32 {
    static CACHE: Mutex<Option<(std::time::Instant, i32)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *cache {
        if at.elapsed() < std::time::Duration::from_secs(600) {
            return v;
        }
    }
    let read = std::process::Command::new("/bin/date").arg("+%z").output().ok().and_then(|o| parse_offset(String::from_utf8_lossy(&o.stdout).trim()));
    let v = read.or(cache.map(|(_, v)| v)).unwrap_or(0);
    *cache = Some((std::time::Instant::now(), v));
    v
}

/// `+0800` → 480。
fn parse_offset(z: &str) -> Option<i32> {
    let (sign, rest) = match z.as_bytes().first()? {
        b'+' => (1, &z[1..]),
        b'-' => (-1, &z[1..]),
        _ => return None,
    };
    if rest.len() != 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m): (i32, i32) = (rest[..2].parse().ok()?, rest[2..].parse().ok()?);
    Some(sign * (h * 60 + m))
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
        // PLOOP（v1.1.1）只用 API 模型：写规范、审查的只能是 API 模型。
        for who in [self.plan_by.as_ref().map(|p| &p.who), self.review_by.as_ref().map(|p| &p.who)].into_iter().flatten() {
            if !who.starts_with("api:") {
                return Err("PLOOP 只用 API 模型：写规范、审查的选 API 模型。".into());
            }
        }
        for who in [Some(&self.who), self.plan_by.as_ref().map(|p| &p.who), self.review_by.as_ref().map(|p| &p.who)].into_iter().flatten() {
            if !who.starts_with("api:") && !who.starts_with("cli:") {
                return Err("定时任务选一个 API 模型或一个 CLI。".into());
            }
            if let Some(engine) = who.strip_prefix("cli:") {
                if crate::gateway::caps::AgentCaps::for_engine(engine).direct_only {
                    return Err("Antigravity 只做直接调用，不进定时任务。".into());
                }
            }
        }
        if !matches!(self.permission.as_str(), "plan" | "edit" | "auto") {
            return Err("权限只有 PLAN、EDIT、AUTO 三档。".into());
        }
        if self.permission == "plan" && (self.plan_by.is_some() || self.review_by.is_some()) {
            return Err("PLAN 只读：PLOOP 的写规范、审查要动手，权限选 EDIT 或 AUTO。".into());
        }
        let bad_time = |t: &str| parse_time(t).is_none().then(|| format!("时间写成 09:30 这样：{t}"));
        match &mut self.when {
            When::Once { date, time } => {
                if let Some(e) = bad_time(time) {
                    return Err(e);
                }
                if parse_date(date).is_none() {
                    return Err(format!("日期写成 2026-12-01 这样：{date}"));
                }
            }
            When::Daily { time, days, .. } => {
                if let Some(e) = bad_time(time) {
                    return Err(e);
                }
                if days.iter().any(|d| !(1..=7).contains(d)) {
                    return Err("星期几写 1 到 7（1 是周一，7 是周日）。".into());
                }
                days.sort_unstable();
                days.dedup();
                if days.len() == 7 {
                    days.clear();
                }
            }
            When::Monthly { day, time } => {
                if let Some(e) = bad_time(time) {
                    return Err(e);
                }
                if !(1..=31).contains(day) {
                    return Err("每月几号：1 到 31。".into());
                }
            }
            When::Interval { hours } => {
                if !(1..=168).contains(hours) {
                    return Err("每隔几小时：1 到 168。".into());
                }
            }
        }
        Ok(self)
    }

    /// 谁来干的那一套。
    pub fn setup(&self) -> Setup {
        Setup { who: self.who.clone(), model: self.model.clone(), effort: self.effort.clone(), plan_by: self.plan_by.clone(), review_by: self.review_by.clone(), delegate: self.delegate.clone() }
    }

    /// `now` 之后的下一次（毫秒）；只跑一次的过了点就没有下一次。时区用 `offset_min`（调用前先 [`Schedule::retime`]）。
    pub fn next_after(&self, now: i64) -> Option<i64> {
        let offset = self.offset_min as i64 * 60_000;
        let local_now = now + offset;
        let today = local_now.div_euclid(DAY_MS);
        let clock = |t: &str| {
            let (h, m) = parse_time(t).unwrap_or((9, 0));
            (h * 60 + m) * 60_000
        };
        match &self.when {
            When::Interval { hours } => {
                let step = *hours as i64 * 3_600_000;
                Some(match self.last_run_at {
                    Some(last) if last + step > now => last + step,
                    _ => now + step,
                })
            }
            When::Once { date, time } => {
                let (y, m, d) = parse_date(date)?;
                let at = days_from_civil(y, m, d) * DAY_MS + clock(time) - offset;
                (at > now).then_some(at)
            }
            When::Daily { time, weekdays, days } => {
                // 1970-01-01 是星期四：天数 + 3 对 7 取余，0 是星期一。
                let allowed = |day: i64| {
                    let wd = (day + 3).rem_euclid(7) as u8 + 1;
                    if !days.is_empty() {
                        days.contains(&wd)
                    } else {
                        !*weekdays || wd <= 5
                    }
                };
                (today..today + 8).map(|day| (day, day * DAY_MS + clock(time))).find(|(day, at)| *at > local_now && allowed(*day)).map(|(_, at)| at - offset)
            }
            When::Monthly { day, time } => {
                let (mut y, mut m, _) = civil_from_days(today);
                for _ in 0..14 {
                    let d = (*day as i64).min(days_in_month(y, m));
                    let at = days_from_civil(y, m, d) * DAY_MS + clock(time);
                    if at > local_now {
                        return Some(at - offset);
                    }
                    (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                }
                None
            }
        }
    }

    /// 换成现在的时区。
    pub fn retime(&mut self) {
        self.offset_min = local_offset_min();
    }

    /// 从 `from`（原定的那一次，已经过了）到 `now` 之间最近的那一次：重复的任务错过多次只认这一次。
    pub fn last_due(&self, from: i64, now: i64) -> i64 {
        let mut at = from;
        let mut probe = self.clone();
        for _ in 0..5000 {
            probe.last_run_at = Some(at);
            match probe.next_after(at) {
                Some(next) if next <= now && next > at => at = next,
                _ => break,
            }
        }
        at
    }

    /// 给人看的一句：什么时候跑。
    pub fn when_text(&self) -> String {
        const WD: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
        match &self.when {
            When::Once { date, time } => format!("{date} {time}（只一次）"),
            When::Daily { time, days, .. } if !days.is_empty() => format!("每{} {time}", days.iter().map(|d| WD[(*d as usize - 1).min(6)]).collect::<Vec<_>>().join("、")),
            When::Daily { time, weekdays: true, .. } => format!("工作日 {time}"),
            When::Daily { time, .. } => format!("每天 {time}"),
            When::Monthly { day, time } => format!("每月 {day} 号 {time}"),
            When::Interval { hours } => format!("每隔 {hours} 小时"),
        }
    }
}

/// 毫秒 → 这台 Mac 的本地时间 `2026-11-23 09:00`。
pub fn local_text(ms: i64) -> String {
    let local = ms + local_offset_min() as i64 * 60_000;
    let (y, m, d) = civil_from_days(local.div_euclid(DAY_MS));
    let mins = local.rem_euclid(DAY_MS) / 60_000;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", mins / 60, mins % 60)
}

/// 现在的本地时间，带星期几和时区（环境块里给模型看，算「明天」「下周一」用）。
pub fn local_now_text() -> String {
    const WD: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let offset = local_offset_min();
    let now = now_ms();
    let day = (now + offset as i64 * 60_000).div_euclid(DAY_MS);
    format!("{} {} (UTC{}{:02}:{:02})", local_text(now), WD[(day + 3).rem_euclid(7) as usize], if offset < 0 { '-' } else { '+' }, offset.abs() / 60, offset.abs() % 60)
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

    /// 同一个对话里这个 CLI 上一次的会话号（定时任务接在对话里跑时续上）。
    fn last_session_ref(&self, session_id: &str, who: &str) -> Option<String> {
        let runs = self.runs(session_id).ok()?;
        runs.iter().rev().filter(|r| r.who.as_deref() == Some(who)).find_map(|r| {
            self.agent_events(&r.id).ok()?.into_iter().find_map(|e| match e.body {
                crate::gateway::event::EventBody::SessionStarted { session_ref: Some(s), .. } => Some(s),
                _ => None,
            })
        })
    }
}

/// 存一个新的或改一个旧的（工具调过来）：校验、排下一次。只跑一次的时间已经过了不收。
pub fn save(db: &Store, schedule: Schedule) -> Result<Schedule, String> {
    let mut s = schedule.checked()?;
    db.workspace(&s.workspace_id).map_err(|_| "没找到这个项目。")?;
    let existing = db.schedules(Some(&s.workspace_id)).map_err(|e| e.to_string())?;
    match existing.iter().find(|x| !s.id.is_empty() && x.id == s.id) {
        Some(old) => {
            s.created_at = old.created_at;
            s.last_run_at = old.last_run_at;
            s.last_session = old.last_session.clone();
            s.last_note = old.last_note.clone();
        }
        None => {
            if !s.id.is_empty() {
                return Err(format!("这个项目里没有 id 是 {} 的定时任务。", s.id));
            }
            if existing.len() >= MAX_PER_PROJECT {
                return Err(format!("一个项目最多 {MAX_PER_PROJECT} 个定时任务，先取消一个。"));
            }
            s.id = format!("sched-{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
            s.created_at = now_ms();
        }
    }
    s.retime();
    s.missed_at = None;
    s.next_at = if s.enabled { s.next_after(now_ms()) } else { None };
    if s.enabled && s.next_at.is_none() {
        return Err("这个时间已经过了，换一个以后的时间。".into());
    }
    db.save_schedule(&s).map_err(|e| e.to_string())?;
    changed();
    Ok(s)
}

/// 取消一个（只认这个项目里的）。
pub fn cancel(db: &Store, workspace_id: &str, id: &str) -> Result<Schedule, String> {
    let found = db.schedules(Some(workspace_id)).map_err(|e| e.to_string())?.into_iter().find(|s| s.id == id).ok_or_else(|| format!("这个项目里没有 id 是 {id} 的定时任务。"))?;
    db.delete_schedule(id).map_err(|e| e.to_string())?;
    changed();
    Ok(found)
}

fn app() -> &'static OnceLock<tauri::AppHandle> {
    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
    &APP
}

/// 定时任务变了（设了、取消了、跑了、错过了）：告诉界面重读。
pub fn changed() {
    if let Some(app) = app().get() {
        let _ = app.emit("schedules-changed", ());
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

/// 现在就发一次（到点了、或者站主在卡片上点了 RUN NOW）：在记着的那个对话里接着发，或者开一个新任务；
/// 事件经 `schedule-event` 推给界面。
pub async fn fire(app: &tauri::AppHandle, schedule: &Schedule) -> Result<Fired, String> {
    let state = app.state::<AppState>();
    let (session_id, workspace_id, fresh, resume) = {
        let db = state.store.lock().map_err(|_| "读不到项目。")?;
        let ws = db.workspace(&schedule.workspace_id).map_err(|_| "这个项目不在列表里了。")?;
        if !std::path::Path::new(&ws.path).is_dir() {
            return Err("这个项目的文件夹不在了。".into());
        }
        // 记着的那个对话还在、没归档就接在里面；不然开新的。
        let kept = schedule.session.as_ref().filter(|sid| db.sessions(&ws.id).is_ok_and(|l| l.iter().any(|s| &s.id == *sid && s.archived_at.is_none())));
        match kept {
            Some(sid) => (sid.clone(), ws.id, false, db.last_session_ref(sid, &schedule.who)),
            None => (db.create_session(&ws.id, Some(format!("⏰ {}", schedule.title))).map_err(|e| e.to_string())?.id, ws.id, true, None),
        }
    };
    // 任务号拿到之前的事件先攒着（任务在后台起，可能比任务号先到），拿到再一起推。
    type Slot = std::sync::Arc<Mutex<(Option<String>, Vec<AgentEvent>)>>;
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
    let started = start_run(&state, schedule, &session_id, resume, out).await;
    let run_id = match started {
        Ok(id) => id,
        Err(e) => {
            // 起不来（没 Key、CLI 不在、项目在忙……）：刚开的空任务删掉，不然每到点留一个空的 ⏰。
            if fresh {
                if let Ok(mut db) = state.store.lock() {
                    let _ = db.delete_session(&session_id);
                }
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

/// 照这个定时任务记着的那一套起一个任务：有写规范或审查走 PLOOP，不然一个 API 模型或一个 CLI（CLI 不委派）。
async fn start_run(state: &AppState, schedule: &Schedule, session_id: &str, resume: Option<String>, out: impl Fn(&AgentEvent) + Send + Sync + 'static) -> Result<String, String> {
    use crate::harness::{run, team};
    let level = run::permission_of(Some(&schedule.permission))?;
    // 委派的模型现在用不了（Key 删了）：不委派，照常跑。
    let delegate = run::delegate_of(state, schedule.delegate.as_deref()).await.unwrap_or(None);
    if schedule.plan_by.is_some() || schedule.review_by.is_some() {
        let implement = team::member_of(state, &TeamPick { who: schedule.who.clone(), model: schedule.model.clone(), effort: schedule.effort.clone() }).await?;
        let spec = match &schedule.plan_by {
            Some(p) => Some(team::member_of(state, p).await?),
            None => None,
        };
        let review = match &schedule.review_by {
            Some(p) => Some(team::member_of(state, p).await?),
            None => None,
        };
        return team::start_team_run(
            team::TeamRun {
                store: state.store.clone(),
                active_runs: state.active_runs.clone(),
                session_id: session_id.to_string(),
                prompt: schedule.prompt.clone(),
                permission: Some(schedule.permission.clone()),
                level,
                cost_cap: cost_cap(state),
                env: crate::harness::tools::bash::shell_env(),
                worktree: false,
                spec,
                implement,
                review,
                delegate,
                images: vec![],
                search: crate::gateway::search::Keys::from_state(state),
            },
            out,
        );
    }
    if let Some(model) = schedule.who.strip_prefix("api:") {
        let provider = crate::gateway::providers::for_model(state, model).await.map_err(|e| e.message)?;
        return run::start_peck_run(
            run::PeckRun {
                store: state.store.clone(),
                active_runs: state.active_runs.clone(),
                session_id: session_id.to_string(),
                prompt: schedule.prompt.clone(),
                who: schedule.who.clone(),
                permission: Some(schedule.permission.clone()),
                level,
                model_id: model.to_string(),
                effort: schedule.effort.clone(),
                cost_cap: cost_cap(state),
                env: crate::harness::tools::bash::shell_env(),
                worktree: false,
                delegate,
                images: vec![],
                search: crate::gateway::search::Keys::from_state(state),
            },
            provider,
            out,
        );
    }
    let engine = schedule.who.trim_start_matches("cli:").to_string();
    let label = crate::harness::history::who_label(Some(&schedule.who));
    let backend = crate::gateway::engines::from_state(state, &engine).await.map_err(|e| e.message)?;
    crate::gateway::runner::start_engine_run(
        crate::gateway::runner::EngineRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id: session_id.to_string(),
            prompt: schedule.prompt.clone(),
            who: schedule.who.clone(),
            permission: Some(schedule.permission.clone()),
            level,
            engine,
            label,
            model: schedule.model.clone(),
            effort: schedule.effort.clone(),
            resume,
            default_account: true,
            worktree: false,
            attachments: vec![],
        },
        backend,
        out,
    )
}

/// 界面设置里的每月花费上限（定时任务的 Peck Loop 也受它管）。
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

/// 记成错过了：`missed_at` 是最近该跑的那一次，下一次往后排（只跑一次的没有下一次，等站主在卡片上点）。
fn mark_missed(s: &mut Schedule, now: i64, note: &str) {
    s.retime();
    let from = s.next_at.unwrap_or(now);
    s.missed_at = Some(s.last_due(from, now));
    s.last_note = Some(note.to_string());
    s.next_at = s.next_after(now);
}

/// 打开 App 时起：关着的时候错过的记一笔（等站主点卡片），然后每 30 秒看一眼到点的。
pub fn start(handle: tauri::AppHandle) {
    let _ = app().set(handle.clone());
    tauri::async_runtime::spawn(async move {
        {
            let state = handle.state::<AppState>();
            let now = now_ms();
            let stale: Vec<Schedule> = state.store.lock().ok().and_then(|db| db.due_schedules(now).ok()).unwrap_or_default();
            for mut s in stale {
                mark_missed(&mut s, now, "App 关着，错过了");
                if let Ok(db) = state.store.lock() {
                    let _ = db.save_schedule(&s);
                }
            }
            changed();
        }
        loop {
            tick(&handle).await;
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

/// 发完以后记一笔：重新读库里现在的那一份再改（发的时候被取消了就不写回来、被改了就不盖掉）。
/// `old_next` 是读出来时的下一次：库里还是它才往后排。只跑一次的跑完就删。
fn record_ran(state: &AppState, id: &str, old_next: Option<i64>, now: i64, note: &str, session: String) {
    let Ok(db) = state.store.lock() else { return };
    let Some(mut cur) = db.schedules(None).ok().and_then(|l| l.into_iter().find(|x| x.id == id)) else { return };
    if matches!(cur.when, When::Once { .. }) {
        let _ = db.delete_schedule(id);
        return;
    }
    cur.last_run_at = Some(now);
    cur.last_session = Some(session);
    cur.last_note = Some(note.to_string());
    cur.missed_at = None;
    if old_next.is_some() && cur.next_at == old_next {
        cur.retime();
        cur.next_at = cur.next_after(now);
    }
    let _ = db.save_schedule(&cur);
}

/// 没发出去、等太久：记成错过（库里的下一次还是读出来时那个才动）。
fn record_missed(state: &AppState, id: &str, old_next: Option<i64>, now: i64, note: &str) {
    let Ok(db) = state.store.lock() else { return };
    let Some(mut cur) = db.schedules(None).ok().and_then(|l| l.into_iter().find(|x| x.id == id)) else { return };
    if cur.next_at != old_next {
        return;
    }
    mark_missed(&mut cur, now, note);
    let _ = db.save_schedule(&cur);
}

async fn tick(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let now = now_ms();
    let due: Vec<Schedule> = state.store.lock().ok().and_then(|db| db.due_schedules(now).ok()).unwrap_or_default();
    let mut touched = false;
    for s in due {
        if busy(&state, &s.workspace_id) {
            if now - s.next_at.unwrap_or(now) > WAIT_BUSY_MS {
                record_missed(&state, &s.id, s.next_at, now, "项目一直有任务在跑，错过了");
                touched = true;
            }
            continue;
        }
        match fire(app, &s).await {
            Ok(f) => record_ran(&state, &s.id, s.next_at, now, "发出去了", f.session_id),
            Err(e) => record_missed(&state, &s.id, s.next_at, now, &format!("没发出去：{e}")),
        }
        touched = true;
    }
    if touched {
        changed();
    }
}

#[tauri::command]
pub async fn schedule_list(state: tauri::State<'_, AppState>) -> Result<Vec<Schedule>, String> {
    let db = state.store.lock().map_err(|_| "读不到定时任务。")?;
    db.schedules(None).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn schedule_delete(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.lock().map_err(|_| "删不了定时任务。")?.delete_schedule(&id).map_err(|e| e.to_string())?;
    changed();
    Ok(())
}

/// 错过的那张卡片：`run` 为真是 RUN NOW（现在补跑一次，不改下一次的时间），否则是 SKIP。只跑一次的两样都删掉。
#[tauri::command]
pub async fn schedule_missed(id: String, run: bool, app: tauri::AppHandle) -> Result<Option<Fired>, String> {
    let state = app.state::<AppState>();
    let s = {
        let db = state.store.lock().map_err(|_| "读不到定时任务。")?;
        db.schedules(None).map_err(|e| e.to_string())?.into_iter().find(|s| s.id == id).ok_or("没找到这个定时任务。")?
    };
    let fired = if run {
        if busy(&state, &s.workspace_id) {
            return Err("这个项目有任务在跑，跑完再点。".into());
        }
        Some(fire(&app, &s).await?)
    } else {
        None
    };
    {
        let db = state.store.lock().map_err(|_| "存不了定时任务。")?;
        if let Some(mut cur) = db.schedules(None).ok().and_then(|l| l.into_iter().find(|x| x.id == id)) {
            if matches!(cur.when, When::Once { .. }) {
                let _ = db.delete_schedule(&id);
            } else {
                cur.missed_at = None;
                if let Some(f) = &fired {
                    cur.last_run_at = Some(now_ms());
                    cur.last_session = Some(f.session_id.clone());
                    cur.last_note = Some("补跑了一次".into());
                } else {
                    cur.last_note = Some("错过的那一次跳过了".into());
                }
                let _ = db.save_schedule(&cur);
            }
        }
    }
    changed();
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
            plan_by: None,
            review_by: None,
            delegate: None,
            permission: "auto".into(),
            when: When::Daily { time: time.into(), weekdays, days: vec![] },
            offset_min,
            enabled: true,
            next_at: None,
            last_run_at: None,
            last_session: None,
            last_note: None,
            created_at: 0,
            session: None,
            missed_at: None,
        }
    }

    /// 东八区的某天某时 → 毫秒。
    fn at8(y: i64, m: i64, d: i64, h: i64, min: i64) -> i64 {
        days_from_civil(y, m, d) * DAY_MS + (h * 60 + min) * 60_000 - 480 * 60_000
    }

    #[test]
    fn next_times_follow_the_local_clock() {
        // 2026-11-13 是星期五。
        let now = at8(2026, 11, 13, 8, 30);
        let s = daily("09:00", false, 480);
        assert_eq!(s.next_after(now), Some(at8(2026, 11, 13, 9, 0)));
        assert_eq!(s.next_after(at8(2026, 11, 13, 9, 1)), Some(at8(2026, 11, 14, 9, 0)));
        // 只在工作日：星期五过了 9 点，下一次是星期一。
        let s = daily("09:00", true, 480);
        assert_eq!(s.next_after(at8(2026, 11, 13, 9, 1)), Some(at8(2026, 11, 16, 9, 0)));
        // 每周一：从星期五算是 16 号；周一 9 点过了是下周一。
        let mut s = daily("09:00", false, 480);
        s.when = When::Daily { time: "09:00".into(), weekdays: false, days: vec![1] };
        assert_eq!(s.next_after(now), Some(at8(2026, 11, 16, 9, 0)));
        assert_eq!(s.next_after(at8(2026, 11, 16, 9, 0)), Some(at8(2026, 11, 23, 9, 0)));
        assert_eq!(s.when_text(), "每周一 09:00");
        // 每隔几小时：从上一次算；错过了从现在算。
        let mut s = daily("09:00", false, 0);
        s.when = When::Interval { hours: 6 };
        assert_eq!(s.next_after(now), Some(now + 6 * 3_600_000));
        s.last_run_at = Some(now - 3_600_000);
        assert_eq!(s.next_after(now), Some(now + 5 * 3_600_000));
    }

    #[test]
    fn once_and_monthly() {
        let mut s = daily("09:00", false, 480);
        s.when = When::Once { date: "2026-12-01".into(), time: "18:30".into() };
        assert_eq!(s.next_after(at8(2026, 11, 22, 10, 0)), Some(at8(2026, 12, 1, 18, 30)));
        assert_eq!(s.next_after(at8(2026, 12, 1, 18, 30)), None, "过了就没有下一次");
        // 每月 31 号：11 月没有 31 号用 30 号；过了是 12 月 31 号；跨年。
        s.when = When::Monthly { day: 31, time: "08:00".into() };
        assert_eq!(s.next_after(at8(2026, 11, 22, 10, 0)), Some(at8(2026, 11, 30, 8, 0)));
        assert_eq!(s.next_after(at8(2026, 11, 30, 8, 0)), Some(at8(2026, 12, 31, 8, 0)));
        assert_eq!(s.next_after(at8(2026, 12, 31, 9, 0)), Some(at8(2027, 1, 31, 8, 0)));
        assert_eq!(s.next_after(at8(2028, 2, 1, 0, 0)), Some(at8(2028, 2, 29, 8, 0)), "闰年");
        assert_eq!(civil_from_days(days_from_civil(2026, 11, 22)), (2026, 11, 22));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(parse_offset("+0800"), Some(480));
        assert_eq!(parse_offset("-0330"), Some(-210));
        assert_eq!(parse_offset("CST"), None);
    }

    #[test]
    fn missing_many_times_keeps_only_the_latest() {
        // 每天 9 点，从 11 月 10 号起错过，22 号 10 点打开：只认 22 号 9 点那一次，下一次是 23 号。
        let mut s = daily("09:00", false, 480);
        s.next_at = Some(at8(2026, 11, 10, 9, 0));
        let now = at8(2026, 11, 22, 10, 0);
        assert_eq!(s.last_due(s.next_at.unwrap(), now), at8(2026, 11, 22, 9, 0));
        // 每隔 6 小时也一样：最近的那一次不晚于现在、离现在不到 6 小时。
        s.when = When::Interval { hours: 6 };
        let last = s.last_due(at8(2026, 11, 10, 9, 0), now);
        assert!(last <= now && now - last < 6 * 3_600_000);
        // 只跑一次的：错过的就是它自己。
        s.when = When::Once { date: "2026-11-10".into(), time: "09:00".into() };
        assert_eq!(s.last_due(at8(2026, 11, 10, 9, 0), now), at8(2026, 11, 10, 9, 0));
        assert_eq!(s.next_after(now), None);
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
        let mut s = daily("09:00", false, 480);
        s.when = When::Daily { time: "09:00".into(), weekdays: false, days: vec![3, 1, 3, 8] };
        assert!(s.clone().checked().is_err());
        s.when = When::Daily { time: "09:00".into(), weekdays: false, days: vec![3, 1, 3] };
        assert_eq!(s.checked().unwrap().when, When::Daily { time: "09:00".into(), weekdays: false, days: vec![1, 3] });
        let mut s = daily("09:00", false, 480);
        s.when = When::Once { date: "2026-02-30".into(), time: "09:00".into() };
        assert!(s.checked().is_err());
        let mut s = daily("09:00", false, 480);
        s.permission = "plan".into();
        s.review_by = Some(TeamPick { who: "cli:codex".into(), model: None, effort: None });
        assert!(s.clone().checked().unwrap_err().contains("PLOOP 只用 API"), "PLOOP 里 CLI 不能审查");
        s.review_by = Some(TeamPick { who: "api:deepseek/deepseek-flash".into(), model: None, effort: None });
        assert!(s.checked().unwrap_err().contains("PLAN"));
        // 旧记录（没有 days、session、missedAt）照样读得出来。
        let old = r#"{"id":"a","workspaceId":"w","title":"t","prompt":"p","who":"cli:codex","permission":"edit","when":{"kind":"daily","time":"09:00","weekdays":true},"offsetMin":480,"enabled":true}"#;
        let s: Schedule = serde_json::from_str(old).unwrap();
        assert_eq!(s.when_text(), "工作日 09:00");
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
