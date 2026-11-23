//! Peck 自己的几样工具（v1.0.2）：以前在界面上点的事，现在在对话里让模型做。
//!
//! - `restore_snapshot`：列这个项目最近的任务快照，按任务号回到它开始前或结束后的样子（整个任务改过的文件，
//!   或其中几个；`scope: "project"` 是整个项目回到那时）。回退哪一档都当高危，站主批了才动。
//! - `schedule_task` / `list_schedules` / `cancel_schedule`：设、看、取消这个项目的定时任务（`schedule.rs`）。
//!
//! Peck Loop 直接调（`loop.rs`），CLI 引擎经本机 MCP 调（`gateway/mcp.rs`），两边是同一套；怎么用写在自带的两个
//! Skill 里（`defaults/skills/`，`skills.rs` 里嵌着）和工具说明里。

use crate::events::RunStatus;
use crate::schedule::{self, Schedule, Setup, When};
use crate::snapshot::{ShadowRepo, UndoFile};
use crate::store::{now_ms, RunRecord, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const RESTORE_SNAPSHOT: &str = "restore_snapshot";
pub const SCHEDULE_TASK: &str = "schedule_task";
pub const LIST_SCHEDULES: &str = "list_schedules";
pub const CANCEL_SCHEDULE: &str = "cancel_schedule";
pub const NAMES: [&str; 4] = [RESTORE_SNAPSHOT, SCHEDULE_TASK, LIST_SCHEDULES, CANCEL_SCHEDULE];

/// 列快照时最多列几次任务。
const LIST_RUNS: usize = 15;

pub const RESTORE_DESCRIPTION: &str = "Task snapshots of this project: Peck saves the project's files before every task starts and after it ends. action=list shows the recent tasks (number, when, what was asked, files changed, which states exist). action=restore puts files back: task=<number>, to=\"before\" (how things were before that task started, i.e. undo it) or \"after\" (how that task left them, i.e. redo it or drop later edits). By default only the files that task changed are touched; paths=[...] narrows it to some of them; scope=\"project\" instead returns the whole project to that moment (this also reverts every later task and any other edits made since). The user's own git history is never touched. Restoring always asks the user to approve, so before calling restore: call list, tell the user in a sentence or two exactly what will be put back (which task, before or after, which files), and get their go-ahead in the conversation. Never restore on your own initiative.";

pub fn restore_schema() -> Value {
    json!({"type":"object","properties":{
        "action":{"type":"string","enum":["list","restore"]},
        "task":{"type":"integer","minimum":1,"description":"restore: the task number from list."},
        "to":{"type":"string","enum":["before","after"],"description":"restore: the state before that task started, or after it ended."},
        "paths":{"type":"array","items":{"type":"string"},"description":"restore: only these files or folders (project-relative). Omit for everything in scope."},
        "scope":{"type":"string","enum":["task","project"],"description":"restore: task (default) = only files that task changed; project = the whole project back to that moment."}
    },"required":["action"]})
}

pub const SCHEDULE_DESCRIPTION: &str = "Create or change a scheduled task in this project: at the set time Peck sends `prompt` as a new request here, unattended (AUTO permission by default; dangerous actions still wait for the user). Turn what the user said into a time rule plus one self-contained sentence: `prompt` must make sense with no other context, because it runs later in a fresh conversation. repeat: once (date + time), daily (time), weekly (days + time; 1=Monday … 7=Sunday), monthly (day_of_month + time), interval (every_hours). Times are the user's local wall-clock time, 24h HH:MM. who: leave it out to use the agent setup the user has selected right now; set it only when the user names a model or CLI (pass the name they said, e.g. \"DeepSeek\" or \"Codex\"). conversation: \"new\" (default: each run opens a new task) or \"this\" (continue in the current conversation; only when the user asks for that). id: pass an existing schedule's id to change it instead of creating one. Before calling, restate the schedule to the user in plain words (when, how often, the sentence, who runs it) and wait for their yes. Schedules only run while Peck is open; a missed run shows the user a card to run or skip.";

pub fn schedule_schema() -> Value {
    json!({"type":"object","properties":{
        "prompt":{"type":"string","description":"The sentence Peck sends at that time. Self-contained, in the user's language."},
        "title":{"type":"string","description":"A short name (a few words)."},
        "repeat":{"type":"string","enum":["once","daily","weekly","monthly","interval"]},
        "time":{"type":"string","description":"HH:MM, 24h, local time. For once / daily / weekly / monthly."},
        "date":{"type":"string","description":"once: YYYY-MM-DD (local date)."},
        "days":{"type":"array","items":{"type":"integer","minimum":1,"maximum":7},"description":"weekly: 1=Monday … 7=Sunday."},
        "day_of_month":{"type":"integer","minimum":1,"maximum":31,"description":"monthly. Months without that day use their last day."},
        "every_hours":{"type":"integer","minimum":1,"maximum":168,"description":"interval."},
        "who":{"type":"string","description":"Only when the user named a model or CLI: the name they said."},
        "permission":{"type":"string","enum":["plan","edit","auto"],"description":"Default auto. Only set it when the user asks."},
        "conversation":{"type":"string","enum":["new","this"],"description":"Default new."},
        "id":{"type":"string","description":"Change this existing schedule instead of creating a new one."}
    },"required":["prompt","repeat"]})
}

pub const LIST_SCHEDULES_DESCRIPTION: &str = "List this project's scheduled tasks: id, when they run, the sentence they send, who runs them, the next run and how the last one went.";
pub const CANCEL_SCHEDULE_DESCRIPTION: &str = "Cancel (delete) one of this project's scheduled tasks by id (get the id from list_schedules). Tell the user which one you are cancelling.";

/// 一个任务里这几样工具要的东西。
pub struct Extras {
    pub store: Arc<Mutex<Store>>,
    pub workspace_id: String,
    pub session_id: String,
    pub run_id: String,
    /// 这个任务是谁在干（设定时任务时默认照它）。
    pub setup: Setup,
}

/// 要放回去的一处：哪个文件夹的影子仓库、照哪棵树、哪些文件（`kind` 是「那时 → 现在」的变化：`create` 是那时没有、要删掉）。
pub struct Unit {
    repo: ShadowRepo,
    tree: String,
    /// 其他文件夹的根（给人看时拼成绝对路径）；项目自己的为空。
    root: Option<PathBuf>,
    files: Vec<UndoFile>,
}

/// 批之前先算好的一次回退。
pub struct RestorePlan {
    pub run: RunRecord,
    pub before: bool,
    pub project: bool,
    pub partial: bool,
    pub units: Vec<Unit>,
    /// 卡片上的标题和明细。
    pub title: String,
    pub detail: String,
}

fn stamp(ms: i64) -> String {
    schedule::local_text(ms)
}

fn status_word(r: &RunRecord) -> &'static str {
    if r.undone_at.is_some() {
        return "undone";
    }
    match r.status {
        RunStatus::Running => "running",
        RunStatus::Succeeded => "done",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "stopped",
    }
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

impl Extras {
    fn db(&self) -> Result<std::sync::MutexGuard<'_, Store>, String> {
        self.store.lock().map_err(|_| "读不到任务记录。".to_string())
    }

    /// 这个项目的任务记录（新的在前）。
    fn runs(&self) -> Result<Vec<RunRecord>, String> {
        let db = self.db()?;
        let mut all: Vec<RunRecord> = db.sessions(&self.workspace_id).map_err(|e| e.to_string())?.iter().flat_map(|s| db.runs(&s.id).unwrap_or_default()).collect();
        all.sort_by_key(|r| std::cmp::Reverse(r.number));
        Ok(all)
    }

    /// `restore_snapshot` 的 list。
    pub fn list_snapshots(&self) -> Result<String, String> {
        let runs = self.runs()?;
        let db = self.db()?;
        let mut lines = vec!["Recent task snapshots in this project (newest first):".to_string()];
        for r in runs.iter().take(LIST_RUNS) {
            let snap = db.snapshot(&r.id).ok().flatten();
            let states = match (&snap, r.id == self.run_id) {
                (_, true) => "this task (still running; nothing to restore yet)".to_string(),
                (None, _) => "no snapshot".to_string(),
                (Some(s), _) if s.latest_tree == s.base_tree => "before only".to_string(),
                (Some(_), _) => "before, after".to_string(),
            };
            let wt = match &r.worktree {
                Some(w) if w.pending() => " · in a separate worktree, not applied yet".to_string(),
                Some(w) if w.discarded_at.is_some() => " · worktree discarded".to_string(),
                _ => String::new(),
            };
            let files = if r.files.is_empty() {
                "no files changed".to_string()
            } else {
                let shown: Vec<&str> = r.files.iter().take(6).map(|f| f.path.as_str()).collect();
                let more = r.files.len().saturating_sub(shown.len());
                format!("{} file(s): {}{}", r.files.len(), shown.join(", "), if more > 0 { format!(", +{more} more") } else { String::new() })
            };
            lines.push(format!("#{} · {} · {} · \"{}\" · {files} · states: {states}{wt}", r.number, stamp(r.started_at), status_word(r), one_line(&r.prompt, 70)));
        }
        if lines.len() == 1 {
            return Ok("这个项目还没有任务快照。".into());
        }
        lines.push("Restore with action=restore, task=<number>, to=before|after (optionally paths=[…] or scope=project).".into());
        Ok(lines.join("\n"))
    }

    /// 回退之前算好：动哪些文件、卡片上写什么。只读（拍一份「现在」的快照，回退前的样子也留着）。
    pub fn plan_restore(&self, input: &Value) -> Result<RestorePlan, String> {
        let number = super::tools::u64_arg(input, "task").ok_or("缺 task：先 action=list 看任务号。")?;
        let before = match super::tools::str_arg(input, "to").unwrap_or("") {
            "before" => true,
            "after" => false,
            _ => return Err("to 写 before（任务开始前）或 after（任务结束后）。".into()),
        };
        let project = match super::tools::str_arg(input, "scope").unwrap_or("task") {
            "task" => false,
            "project" => true,
            other => return Err(format!("scope 只有 task、project（收到 {other}）。")),
        };
        let wanted: Vec<String> = input
            .get("paths")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(|p| p.trim().trim_start_matches("./").trim_end_matches('/').to_string()).filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        let (run, record) = {
            let db = self.db()?;
            let run = self.runs_locked(&db)?.into_iter().find(|r| r.number == number).ok_or_else(|| format!("这个项目里没有任务 {number}。"))?;
            let record = crate::snapshot::saved_snapshot(&run.id).or(db.snapshot(&run.id).map_err(|e| e.to_string())?).ok_or_else(|| format!("任务 {number} 没有快照，回不去。"))?;
            (run, record)
        };
        if run.id == self.run_id || run.status == RunStatus::Running {
            return Err("这个任务还在跑，没有能回去的快照。".into());
        }
        if let Some(wt) = &run.worktree {
            if wt.pending() {
                return Err(format!("任务 {number} 在独立工作区里，还没搬回项目：项目里没有它的改动，不用回退（不要就让站主点 DISCARD）。"));
            }
            if wt.discarded_at.is_some() {
                return Err(format!("任务 {number} 的独立工作区已经丢掉了，项目里没有它的改动。"));
            }
        }
        if !record.workspace_path.is_dir() {
            return Err("项目文件夹不在了。".into());
        }
        if !before && record.latest_tree == record.base_tree {
            return Err(format!("任务 {number} 没有结束后的快照（没做完就停了）；只能回到它开始前。"));
        }
        let label = format!("{}-restore-{}", self.run_id, now_ms());
        let mut units = Vec::new();
        units.push(unit_for(&record.workspace_path, &record.shadow_dir, &record.base_tree, &record.latest_tree, before, project, None, &label)?);
        for e in &record.extras {
            if e.root.is_dir() {
                units.push(unit_for(&e.root, &e.shadow_dir, &e.base_tree, &e.latest_tree, before, project, Some(e.root.clone()), &label)?);
            }
        }
        let mut missing: Vec<String> = Vec::new();
        if !wanted.is_empty() {
            let shown = |u: &Unit, f: &UndoFile| shown_path(u.root.as_deref(), &f.path);
            for w in &wanted {
                let hit = units.iter().any(|u| u.files.iter().any(|f| path_matches(&shown(u, f), &f.path, w)));
                if !hit {
                    missing.push(w.clone());
                }
            }
            for u in &mut units {
                let root = u.root.clone();
                u.files.retain(|f| wanted.iter().any(|w| path_matches(&shown_path(root.as_deref(), &f.path), &f.path, w)));
            }
        }
        units.retain(|u| !u.files.is_empty());
        let total: usize = units.iter().map(|u| u.files.len()).sum();
        if total == 0 {
            return Err(if wanted.is_empty() {
                format!("没有要放回去的：任务 {number} 改过的文件现在就是它{}的样子。", if before { "开始前" } else { "结束后" })
            } else {
                format!("这几个路径里没有要放回去的（不是任务 {number} 改的，或者现在就是那时的样子）：{}", wanted.join("、"))
            });
        }
        let point = if before { "开始前" } else { "结束后" };
        let title = format!(
            "高危：{}回到任务 {number} {point}（{total} 个文件）",
            if project { "整个项目" } else if wanted.is_empty() { "" } else { "部分文件" }
        );
        let mut detail = vec![format!("任务 {number}（{}）：{}", stamp(run.started_at), one_line(&run.prompt, 80))];
        if project {
            detail.push("整个项目回到那时：之后每一次任务的改动、你自己后来改的，都会一起放回去。".into());
        }
        let mut listed = 0;
        for u in &units {
            for f in &u.files {
                if listed == 40 {
                    break;
                }
                listed += 1;
                detail.push(format!("{}  {}", if f.kind == "create" { "删掉" } else { "写回" }, shown_path(u.root.as_deref(), &f.path)));
            }
        }
        if total > listed {
            detail.push(format!("……还有 {} 个", total - listed));
        }
        if !missing.is_empty() {
            detail.push(format!("不动（不在范围里，或现在就是那时的样子）：{}", missing.join("、")));
        }
        detail.push("回退前的样子也存了一份快照；你自己的 Git 不动。".into());
        Ok(RestorePlan { run, before, project, partial: !wanted.is_empty(), units, title, detail: detail.join("\n") })
    }

    fn runs_locked(&self, db: &Store) -> Result<Vec<RunRecord>, String> {
        Ok(db.sessions(&self.workspace_id).map_err(|e| e.to_string())?.iter().flat_map(|s| db.runs(&s.id).unwrap_or_default()).collect())
    }

    /// 站主批了：照算好的放回去，记到任务记录上（整个任务回到开始前记「已撤销」，回到结束后去掉这个记号）。
    pub fn restore(&self, plan: &RestorePlan) -> Result<String, String> {
        let (mut written, mut removed) = (0, 0);
        for u in &plan.units {
            u.repo.restore_paths(&u.tree, &u.files)?;
            removed += u.files.iter().filter(|f| f.kind == "create").count();
            written += u.files.iter().filter(|f| f.kind != "create").count();
        }
        if !plan.partial {
            if let Ok(mut db) = self.store.lock() {
                let n = plan.run.number;
                let touched: Vec<RunRecord> = if plan.project {
                    self.runs_locked(&db).unwrap_or_default().into_iter().filter(|r| r.number >= n && r.id != self.run_id && r.status != RunStatus::Running).collect()
                } else {
                    vec![plan.run.clone()]
                };
                for mut r in touched {
                    // 整个项目回到任务 N 结束后：N 自己算还在，后面的算撤销了。
                    let undone = plan.before || (plan.project && r.number > n);
                    let next = if undone { r.undone_at.or(Some(now_ms())) } else { None };
                    if r.undone_at != next {
                        r.undone_at = next;
                        let _ = db.save_run(&r);
                    }
                }
            }
        }
        Ok(format!(
            "放回去了：{}回到任务 {} {}，写回 {written} 个文件、删掉 {removed} 个。这次回退算在当前这个任务的改动里（回退前的样子也有快照，说一声能再放回来）。",
            if plan.project { "整个项目" } else if plan.partial { "这几个文件" } else { "它改过的文件" },
            plan.run.number,
            if plan.before { "开始前" } else { "结束后" },
        ))
    }

    /// `schedule_task`：设一个新的或改一个旧的。
    pub fn schedule_task(&self, input: &Value) -> Result<String, String> {
        let s = |k: &str| super::tools::str_arg(input, k).map(str::trim).filter(|v| !v.is_empty());
        let time = || s("time").map(str::to_string).ok_or_else(|| "缺 time（HH:MM，24 小时制）。".to_string());
        let when = match s("repeat").unwrap_or("") {
            "once" => When::Once { date: s("date").ok_or("只跑一次要写 date（YYYY-MM-DD）。")?.to_string(), time: time()? },
            "daily" => When::Daily { time: time()?, weekdays: false, days: vec![] },
            "weekly" => {
                let days: Vec<u8> = input.get("days").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|d| d.min(255) as u8).collect()).unwrap_or_default();
                if days.is_empty() {
                    return Err("每周要写 days（1 是周一 … 7 是周日）。".into());
                }
                When::Daily { time: time()?, weekdays: false, days }
            }
            "monthly" => When::Monthly { day: super::tools::u64_arg(input, "day_of_month").ok_or("每月要写 day_of_month（1–31）。")?.min(255) as u8, time: time()? },
            "interval" => When::Interval { hours: super::tools::u64_arg(input, "every_hours").ok_or("每隔几小时要写 every_hours（1–168）。")?.min(100_000) as u32 },
            other => return Err(format!("repeat 只有 once、daily、weekly、monthly、interval（收到 {other:?}）。")),
        };
        let db = self.db()?;
        let old = match s("id") {
            Some(id) => Some(db.schedules(Some(&self.workspace_id)).map_err(|e| e.to_string())?.into_iter().find(|x| x.id == id).ok_or_else(|| format!("这个项目里没有 id 是 {id} 的定时任务（list_schedules 看有哪些）。"))?),
            None => None,
        };
        // 谁来干：点了名的用点名的那一个（只它自己，不带接力、审查）；改旧的没点名就不动；新的照现在这一套。
        let setup = match (s("who"), &old) {
            (Some(name), _) => resolve_who(name)?,
            (None, Some(o)) => o.setup(),
            (None, None) => self.setup.clone(),
        };
        let session = match (s("conversation"), &old) {
            (Some("this"), _) => Some(self.session_id.clone()),
            (Some("new"), _) => None,
            (Some(other), _) => return Err(format!("conversation 只有 new、this（收到 {other}）。")),
            (None, Some(o)) => o.session.clone(),
            (None, None) => None,
        };
        let saved = schedule::save(
            &db,
            Schedule {
                id: old.as_ref().map(|o| o.id.clone()).unwrap_or_default(),
                workspace_id: self.workspace_id.clone(),
                title: s("title").map(str::to_string).or(old.as_ref().map(|o| o.title.clone())).unwrap_or_default(),
                prompt: s("prompt").unwrap_or("").to_string(),
                who: setup.who,
                model: setup.model,
                effort: setup.effort,
                plan_by: setup.plan_by,
                review_by: setup.review_by,
                delegate: setup.delegate,
                permission: s("permission").map(str::to_string).or(old.as_ref().map(|o| o.permission.clone())).unwrap_or_else(|| "auto".into()),
                when,
                offset_min: 0,
                enabled: true,
                next_at: None,
                last_run_at: None,
                last_session: None,
                last_note: None,
                created_at: 0,
                session,
                missed_at: None,
            },
        )?;
        Ok(format!("{}了。\n{}", if old.is_some() { "改好" } else { "设好" }, describe(&saved)))
    }

    /// `list_schedules`。
    pub fn list_schedules(&self) -> Result<String, String> {
        let list = self.db()?.schedules(Some(&self.workspace_id)).map_err(|e| e.to_string())?;
        if list.is_empty() {
            return Ok("这个项目没有定时任务。".into());
        }
        Ok(list.iter().map(describe).collect::<Vec<_>>().join("\n\n"))
    }

    /// `cancel_schedule`。
    pub fn cancel_schedule(&self, input: &Value) -> Result<String, String> {
        let id = super::tools::str_arg(input, "id").map(str::trim).filter(|v| !v.is_empty()).ok_or("缺 id（list_schedules 看有哪些）。")?;
        let gone = schedule::cancel(&*self.db()?, &self.workspace_id, id)?;
        Ok(format!("取消了「{}」（{}）。", gone.title, gone.when_text()))
    }
}

/// 一个文件夹里要放回去的文件：照 `tree`（开始前或结束后）和现在比；`project` 为假时只留这个任务改过的。
#[allow(clippy::too_many_arguments)]
fn unit_for(root: &Path, shadow: &Path, base: &str, latest: &str, before: bool, project: bool, extra: Option<PathBuf>, label: &str) -> Result<Unit, String> {
    let repo = ShadowRepo::with_shadow_dir(root, shadow)?;
    let now_tree = repo.create_snapshot(label)?;
    let tree = if before { base } else { latest }.to_string();
    let mut files: Vec<UndoFile> = repo.diff_changeset(&tree, &now_tree)?.files.into_iter().map(|f| UndoFile { path: f.path, kind: f.kind, changed_since: false }).collect();
    if !project {
        // 没有步骤收尾过（刚开始就被打断）时，拿现在的样子当任务后的样子。
        let after = if latest == base { now_tree.as_str() } else { latest };
        let touched: std::collections::HashSet<String> = repo.diff_changeset(base, after)?.files.into_iter().map(|f| f.path).collect();
        files.retain(|f| touched.contains(&f.path));
    }
    Ok(Unit { repo, tree, root: extra, files })
}

fn shown_path(root: Option<&Path>, rel: &str) -> String {
    match root {
        Some(r) => r.join(rel).to_string_lossy().into_owned(),
        None => rel.to_string(),
    }
}

/// 模型给的路径认不认这个文件：一样，或者是它上面的文件夹（其他文件夹里的用绝对路径）。
fn path_matches(shown: &str, rel: &str, wanted: &str) -> bool {
    [shown, rel].iter().any(|p| *p == wanted || p.strip_prefix(wanted).is_some_and(|rest| rest.starts_with('/')))
}

/// 站主话里点的名（「DeepSeek」「用 Codex 跑」）→ 目录里的一个 API 模型或 CLI。认不出、不止一个都把能选的列回去。
pub fn resolve_who(name: &str) -> Result<Setup, String> {
    let cat = crate::gateway::catalog::catalog();
    let raw = name.trim();
    let key = norm(raw.strip_prefix("api:").or(raw.strip_prefix("cli:")).unwrap_or(raw));
    let one = |who: String| Ok(Setup { who, ..Default::default() });
    let engines: Vec<&crate::gateway::catalog::EngineSpec> = cat.engines.iter().filter(|e| !crate::gateway::caps::AgentCaps::for_engine(&e.id).direct_only).collect();
    if let Some(e) = engines.iter().find(|e| norm(&e.id) == key || norm(&e.label) == key) {
        return one(format!("cli:{}", e.id));
    }
    if let Some(m) = cat.models.iter().find(|m| norm(&m.id) == key || norm(&m.label) == key || norm(&m.api_model) == key) {
        return one(format!("api:{}", m.id));
    }
    if key.is_empty() {
        return Err("who 是空的。".into());
    }
    let mut hits: Vec<(String, String)> = engines.iter().filter(|e| norm(&e.label).contains(&key) || norm(&e.id).contains(&key)).map(|e| (format!("cli:{}", e.id), e.label.clone())).collect();
    hits.extend(cat.models.iter().filter(|m| norm(&m.label).contains(&key) || norm(&m.id).contains(&key)).map(|m| (format!("api:{}", m.id), m.label.clone())));
    match hits.len() {
        1 => one(hits.remove(0).0),
        0 => Err(format!("没认出「{raw}」是哪个模型或 CLI。能选的 CLI：{}；API 模型按名字写（比如 {}）。", engines.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join("、"), cat.models.iter().take(4).map(|m| m.label.as_str()).collect::<Vec<_>>().join("、"))),
        _ => Err(format!("「{raw}」对得上不止一个，问站主要哪一个，再把全名传进来：{}。", hits.iter().take(12).map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("、"))),
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 给模型看的一个定时任务。
fn describe(s: &Schedule) -> String {
    let label = |who: &str| super::history::who_label(Some(who));
    let mut who = label(&s.who);
    if let Some(p) = &s.plan_by {
        who = format!("接力 {} → {who}", label(&p.who));
    }
    if let Some(r) = &s.review_by {
        who.push_str(&format!("，{} 审查", label(&r.who)));
    }
    let mut lines = vec![
        format!("id {} ·「{}」", s.id, s.title),
        format!("什么时候：{}；下一次 {}", s.when_text(), s.next_at.map(schedule::local_text).unwrap_or_else(|| "没有了".into())),
        format!("到点发的那句话：{}", s.prompt),
        format!("谁来干：{who} · 权限 {} · {}", s.permission.to_uppercase(), if s.session.is_some() { "接在设定的那个对话里" } else { "每次开新任务" }),
    ];
    if let Some(at) = s.missed_at {
        lines.push(format!("错过了 {} 那一次，等站主在卡片上点 RUN NOW 或 SKIP。", schedule::local_text(at)));
    }
    if let Some(n) = &s.last_note {
        lines.push(format!("上一次：{n}{}", s.last_run_at.map(|t| format!("（{}）", schedule::local_text(t))).unwrap_or_default()));
    }
    lines.join("\n")
}

/// 测试里搭一个带任务快照的项目（这里、Peck Loop、本机 MCP 的测试都用）。
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::flow::DispatchPlan;
    pub use crate::harness::tools::testing::TempProject;
    use crate::snapshot::{record_run_base, record_run_finish};

    pub struct World {
        pub p: TempProject,
        pub root: PathBuf,
        pub store: Arc<Mutex<Store>>,
        pub session: String,
    }

    pub fn world() -> World {
        let root = std::env::temp_dir().join(format!("peck-extras-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let p = TempProject::new();
        let store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace { id: "wx".into(), path: p.dir.to_string_lossy().into_owned(), name: "x".into(), last_opened_at: 0, git_branch: None, hidden: false, settings: Default::default() };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session("wx", None).unwrap().id;
        World { p, root, store: Arc::new(Mutex::new(store)), session }
    }

    /// 记一次做完的任务：`change` 里改文件，前后各拍一份快照。
    pub fn task(w: &World, number: u64, change: impl FnOnce(&TempProject)) -> RunRecord {
        let repo = ShadowRepo::with_shadow_dir(&w.p.dir, &w.root.join("shadow")).unwrap();
        let id = format!("run-{number}");
        let plan = DispatchPlan { steps: vec![] };
        let mut db = w.store.lock().unwrap();
        let mut record = db.create_run(&w.session, &format!("第 {number} 次"), number, &plan).unwrap();
        let base = repo.create_snapshot(&format!("{id}-base")).unwrap();
        record_run_base(&id, &repo, base.clone());
        change(&w.p);
        let after = repo.create_snapshot(&format!("{id}-after")).unwrap();
        record_run_finish(&id, after.clone());
        record.status = RunStatus::Succeeded;
        record.files = repo.diff_changeset(&base, &after).unwrap().files;
        db.save_run(&record).unwrap();
        record
    }

    pub fn extras(w: &World, run_id: &str) -> Extras {
        Extras { store: w.store.clone(), workspace_id: "wx".into(), session_id: w.session.clone(), run_id: run_id.into(), setup: Setup { who: "api:deepseek/deepseek-flash".into(), ..Default::default() } }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{task, world, World};
    use super::*;

    fn extras(w: &World) -> Extras {
        super::testing::extras(w, "run-now")
    }

    #[test]
    fn snapshots_list_and_restore_before_after_and_partly() {
        let w = world();
        w.p.write("a.txt", "a0\n").write("b.txt", "b0\n").write("keep.txt", "k0\n");
        task(&w, 920_001, |p| {
            p.write("a.txt", "a1\n").write("b.txt", "b1\n").write("new.txt", "n1\n");
        });
        // 做完以后站主自己改了别的文件：按任务回退不动它。
        w.p.write("keep.txt", "k-mine\n");
        let x = extras(&w);
        let list = x.list_snapshots().unwrap();
        assert!(list.contains("#920001") && list.contains("a.txt") && list.contains("before, after"), "{list}");

        // 只回退一个文件
        let plan = x.plan_restore(&json!({"task": 920_001, "to": "before", "paths": ["a.txt", "nope.txt"]})).unwrap();
        assert!(plan.partial && plan.title.contains("部分文件") && plan.detail.contains("写回  a.txt") && plan.detail.contains("nope.txt"), "{}\n{}", plan.title, plan.detail);
        x.restore(&plan).unwrap();
        assert_eq!((w.p.read("a.txt"), w.p.read("b.txt")), ("a0\n".into(), "b1\n".into()));
        assert!(w.store.lock().unwrap().run("run-920001").unwrap().undone_at.is_none(), "只回退一部分不算撤销");

        // 整个任务回到开始前：新建的删掉，站主自己改的不动，记成已撤销
        let plan = x.plan_restore(&json!({"task": 920_001, "to": "before"})).unwrap();
        assert!(plan.detail.contains("删掉  new.txt") && !plan.detail.contains("a.txt"), "{}", plan.detail);
        let said = x.restore(&plan).unwrap();
        assert!(said.contains("写回 1 个") && said.contains("删掉 1 个"), "{said}");
        assert_eq!((w.p.read("b.txt"), w.p.read("keep.txt")), ("b0\n".into(), "k-mine\n".into()));
        assert!(!w.p.dir.join("new.txt").exists());
        assert!(w.store.lock().unwrap().run("run-920001").unwrap().undone_at.is_some());
        assert!(x.plan_restore(&json!({"task": 920_001, "to": "before"})).err().unwrap().contains("没有要放回去的"));

        // 再回到结束后（重做）：记号去掉
        let plan = x.plan_restore(&json!({"task": 920_001, "to": "after"})).unwrap();
        x.restore(&plan).unwrap();
        assert_eq!((w.p.read("a.txt"), w.p.read("new.txt"), w.p.read("keep.txt")), ("a1\n".into(), "n1\n".into(), "k-mine\n".into()));
        assert!(w.store.lock().unwrap().run("run-920001").unwrap().undone_at.is_none());

        // 整个项目回到那时：站主后来改的也回去
        let plan = x.plan_restore(&json!({"task": 920_001, "to": "before", "scope": "project"})).unwrap();
        assert!(plan.title.contains("整个项目") && plan.detail.contains("keep.txt"));
        x.restore(&plan).unwrap();
        assert_eq!((w.p.read("a.txt"), w.p.read("keep.txt")), ("a0\n".into(), "k0\n".into()));

        assert!(x.plan_restore(&json!({"task": 5, "to": "before"})).err().unwrap().contains("没有任务 5"));
        assert!(x.plan_restore(&json!({"task": 920_001, "to": "sideways"})).is_err());
        let _ = std::fs::remove_dir_all(&w.root);
    }

    #[test]
    fn schedules_are_set_listed_changed_and_cancelled_from_the_conversation() {
        let w = world();
        let x = extras(&w);
        assert!(x.list_schedules().unwrap().contains("没有定时任务"));
        // 「每周一早上九点帮我检查依赖更新」
        let said = x.schedule_task(&json!({"prompt": "检查依赖有没有更新，列出能升的", "title": "依赖更新", "repeat": "weekly", "days": [1], "time": "09:00"})).unwrap();
        assert!(said.contains("设好了") && said.contains("每周一 09:00") && said.contains("AUTO") && said.contains("每次开新任务") && said.contains("DeepSeek"), "{said}");
        let all = w.store.lock().unwrap().schedules(Some("wx")).unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!((s.who.as_str(), s.permission.as_str(), s.session.clone()), ("api:deepseek/deepseek-flash", "auto", None));
        assert!(s.next_at.is_some_and(|t| t > now_ms()));
        // 点名、接在这个对话里、改旧的
        let said = x.schedule_task(&json!({"id": s.id, "prompt": "检查依赖有没有更新", "repeat": "daily", "time": "08:30", "who": "codex", "conversation": "this"})).unwrap();
        assert!(said.contains("改好了") && said.contains("每天 08:30") && said.contains("Codex") && said.contains("接在设定的那个对话里"), "{said}");
        let all = w.store.lock().unwrap().schedules(Some("wx")).unwrap();
        assert_eq!((all.len(), all[0].who.as_str(), all[0].session.as_deref(), all[0].title.as_str()), (1, "cli:codex", Some(w.session.as_str()), "依赖更新"));
        // 写错的
        assert!(x.schedule_task(&json!({"prompt": "x", "repeat": "weekly", "time": "09:00"})).unwrap_err().contains("days"));
        assert!(x.schedule_task(&json!({"prompt": "x", "repeat": "once", "date": "2020-01-01", "time": "09:00"})).unwrap_err().contains("已经过了"));
        assert!(x.schedule_task(&json!({"prompt": "x", "repeat": "daily", "time": "09:00", "who": "antigravity"})).is_err(), "只做直接调用的不进定时任务");
        assert!(x.schedule_task(&json!({"prompt": "x", "repeat": "daily", "time": "09:00", "who": "没有这个模型"})).unwrap_err().contains("没认出"));
        assert!(x.schedule_task(&json!({"id": "nope", "prompt": "x", "repeat": "daily", "time": "09:00"})).is_err());
        assert!(x.list_schedules().unwrap().contains(&all[0].id));
        assert!(x.cancel_schedule(&json!({"id": all[0].id})).unwrap().contains("取消了"));
        assert!(w.store.lock().unwrap().schedules(Some("wx")).unwrap().is_empty());
        assert!(x.cancel_schedule(&json!({"id": all[0].id})).is_err());
        let _ = std::fs::remove_dir_all(&w.root);
    }

    #[test]
    fn names_the_user_says_resolve_to_one_model_or_cli() {
        assert_eq!(resolve_who("Codex").unwrap().who, "cli:codex");
        assert_eq!(resolve_who("cli:claude-code").unwrap().who, "cli:claude-code");
        assert_eq!(resolve_who("deepseek/deepseek-flash").unwrap().who, "api:deepseek/deepseek-flash");
        // 「DeepSeek」对得上好几个模型：让模型去问站主。
        let many = resolve_who("deepseek");
        assert!(many.as_ref().is_ok_and(|s| s.who.starts_with("api:deepseek/")) || many.unwrap_err().contains("不止一个"));
    }
}
