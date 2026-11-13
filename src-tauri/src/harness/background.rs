//! 后台命令通道（v0.6 Phase 8）：不再一刀切 120 秒超时。
//!
//! - **模型起的**（`bash_background` 工具）：起了就回来，模型接着干活，回头再读它的输出、停掉它；
//!   这个任务结束时一起停（常驻的不留，和 `bash_run` 一样）。同一个任务最多同时 4 个。
//! - **常驻的 dev server**（`.peck/verify.toml` 的 `[[server]]`）：站主在中栏起、停，跨任务一直在，
//!   退出 Peck 才停；`autostart = true` 的发任务时没在跑就先起。模型能读它的输出，不能停它。
//!
//! 沙箱、环境、进程组照 `bash_run`（[`super::tools::bash::run_command`]）。输出整份写进日志文件，内存里留最近 512KB。

use super::tools::bash::{self, Exec, Sandbox};
use super::tools::{clip_bytes, tail_bytes, ToolContext};
use crate::gateway::event::{BackgroundKind, BackgroundStatus, EventBody, PermissionLevel};
use crate::store::now_ms;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::{watch, Notify};

/// 内存里每个后台命令留这么多输出。
const RING: usize = 512 * 1024;
/// 一次读给模型的新输出最多这么多（多了留开头结尾，完整的在日志文件）。
const READ_LIMIT: usize = 8 * 1024;
/// 模型起的后台命令最长跑这么久（实际上就是不限；任务结束一起停）。
const LONGEST: Duration = Duration::from_secs(12 * 3600);
/// 常驻的 dev server 不限时：退出 Peck 才停。
const FOREVER: Duration = Duration::from_secs(365 * 24 * 3600);
/// 一个任务里同时在跑的后台命令最多几个。
pub const PER_RUN: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgInfo {
    pub id: String,
    pub name: String,
    pub command: String,
    pub kind: BackgroundKind,
    pub status: BackgroundStatus,
    pub exit_code: Option<i32>,
    /// 起好了（server 配了 `ready` 时，看到那段文字才算）。
    pub ready: bool,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// 谁起的：任务 id；常驻的为空。
    pub owner: Option<String>,
    /// 完整输出的日志文件。
    pub log: String,
}

struct BgState {
    status: BackgroundStatus,
    exit_code: Option<i32>,
    ready: bool,
    ended_at: Option<i64>,
    /// 最近的输出（最多 [`RING`]）。
    tail: String,
    /// 一共出了多少字节（模型读到哪了按它算）。
    total: usize,
    /// 模型上次读到哪。
    cursor: usize,
}

pub struct BgProc {
    pub id: String,
    pub workspace: PathBuf,
    pub owner: Option<String>,
    pub name: String,
    pub command: String,
    pub kind: BackgroundKind,
    pub started_at: i64,
    pub log: PathBuf,
    ready_text: Option<String>,
    state: Mutex<BgState>,
    cancel: watch::Sender<bool>,
    changed: Notify,
}

type OnUpdate = Arc<dyn Fn(&BgInfo) + Send + Sync>;

fn registry() -> &'static Mutex<HashMap<String, Arc<BgProc>>> {
    static R: OnceLock<Mutex<HashMap<String, Arc<BgProc>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

impl BgProc {
    pub fn info(&self) -> BgInfo {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        BgInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            command: self.command.clone(),
            kind: self.kind,
            status: s.status,
            exit_code: s.exit_code,
            ready: s.ready,
            started_at: self.started_at,
            ended_at: s.ended_at,
            owner: self.owner.clone(),
            log: self.log.to_string_lossy().into_owned(),
        }
    }

    pub fn running(&self) -> bool {
        self.state.lock().map(|s| s.status == BackgroundStatus::Running).unwrap_or(false)
    }

    fn push(&self, text: &str) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.total += text.len();
        s.tail.push_str(text);
        if s.tail.len() > RING {
            let cut = s.tail.len() - RING;
            let mut at = cut;
            while !s.tail.is_char_boundary(at) {
                at += 1;
            }
            s.tail.drain(..at);
        }
        if !s.ready {
            if let Some(r) = &self.ready_text {
                if s.tail.contains(r.as_str()) {
                    s.ready = true;
                }
            }
        }
        drop(s);
        self.changed.notify_waiters();
    }

    /// 最近的输出（最多 `limit` 字节，界面用）。
    pub fn tail(&self, limit: usize) -> String {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        tail_bytes(&s.tail, limit).to_string()
    }

    /// 给模型读：上次读到以后的新输出，多了留开头结尾。读完往后挪。
    pub fn read_new(&self) -> String {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let new_bytes = s.total - s.cursor;
        let available = s.tail.len().min(new_bytes);
        let start = s.tail.len() - available;
        let mut at = start;
        while !s.tail.is_char_boundary(at) {
            at += 1;
        }
        let fresh = s.tail[at..].to_string();
        s.cursor = s.total;
        let lost = new_bytes.saturating_sub(available);
        drop(s);
        let mut out = String::new();
        if lost > 0 {
            out.push_str(&format!("[前面还有 {} KB 没留在内存里，完整的在 {}]\n", lost / 1024 + 1, self.log.display()));
        }
        if fresh.len() > READ_LIMIT {
            out.push_str(&format!(
                "{}\n…（中间省略，完整的在 {}）…\n{}",
                clip_bytes(&fresh, 2 * 1024),
                self.log.display(),
                tail_bytes(&fresh, 6 * 1024)
            ));
        } else {
            out.push_str(&fresh);
        }
        out
    }

    pub fn status_line(&self) -> String {
        let i = self.info();
        let secs = (i.ended_at.unwrap_or_else(now_ms) - i.started_at).max(0) / 1000;
        match i.status {
            BackgroundStatus::Running => format!("「{}」在跑（{secs}s，id {}）{}", i.name, i.id, if i.ready || self.ready_text.is_none() { "" } else { "，还没起好" }),
            BackgroundStatus::Exited => format!("「{}」结束了，退出码 {}（跑了 {secs}s，id {}）", i.name, i.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "未知".into()), i.id),
            BackgroundStatus::Stopped => format!("「{}」已停（跑了 {secs}s，id {}）", i.name, i.id),
        }
    }

    pub fn stop(&self) {
        let _ = self.cancel.send(true);
    }

    /// 等新输出或者结束，最多 `wait`。
    pub async fn wait_change(&self, wait: Duration) {
        if !self.running() || wait.is_zero() {
            return;
        }
        let notified = self.changed.notified();
        let _ = tokio::time::timeout(wait, notified).await;
    }
}

/// 起一个后台命令要的。
pub struct Spec {
    pub workspace: PathBuf,
    pub owner: Option<String>,
    pub name: String,
    pub command: String,
    /// 相对项目根。
    pub cwd: Option<String>,
    pub kind: BackgroundKind,
    pub ready: Option<String>,
    pub sandbox: Sandbox,
    pub env: Vec<(String, String)>,
    pub runtime_dir: PathBuf,
    /// 日志放哪（模型起的放这个任务的输出目录，`read_file` 读得到）。
    pub log_dir: PathBuf,
}

pub fn start(spec: Spec, on_update: Option<OnUpdate>) -> Result<Arc<BgProc>, String> {
    let ctx = ToolContext::new(&spec.workspace, PermissionLevel::Auto, spec.log_dir.clone(), spec.runtime_dir.clone(), spec.env.clone())?;
    let cwd = super::tools::resolve(&ctx.root, spec.cwd.as_deref().unwrap_or("."))?;
    if !cwd.is_dir() {
        return Err(format!("{} 不是文件夹。", ctx.rel(&cwd)));
    }
    let short = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let id = match spec.kind {
        BackgroundKind::Server => format!("srv-{}-{short}", spec.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>()),
        BackgroundKind::Command => format!("bg-{short}"),
    };
    std::fs::create_dir_all(&spec.log_dir).map_err(|e| format!("建不了日志目录：{e}"))?;
    let log = spec.log_dir.join(format!("{id}.log"));
    let (cancel, cancel_rx) = watch::channel(false);
    let proc_ = Arc::new(BgProc {
        id: id.clone(),
        workspace: ctx.root.clone(),
        owner: spec.owner.clone(),
        name: spec.name.clone(),
        command: spec.command.clone(),
        kind: spec.kind,
        started_at: now_ms(),
        log: log.clone(),
        ready_text: spec.ready.clone().filter(|r| !r.trim().is_empty()),
        state: Mutex::new(BgState {
            status: BackgroundStatus::Running,
            exit_code: None,
            ready: spec.ready.as_deref().is_none_or(|r| r.trim().is_empty()),
            ended_at: None,
            tail: String::new(),
            total: 0,
            cursor: 0,
        }),
        cancel,
        changed: Notify::new(),
    });
    registry().lock().map_err(|_| "后台命令表坏了。")?.insert(id.clone(), proc_.clone());
    if let Some(f) = &on_update {
        f(&proc_.info());
    }
    let p = proc_.clone();
    let sandbox = spec.sandbox;
    let timeout = if spec.kind == BackgroundKind::Server { FOREVER } else { LONGEST };
    tokio::spawn(async move {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
        let writer = p.clone();
        let result = bash::run_command(
            &ctx,
            Exec { command: &p.command, cwd: &cwd, timeout, sandbox, keep_bytes: 0, stream_bytes: usize::MAX },
            cancel_rx,
            move |text| {
                if let Some(f) = file.as_mut() {
                    let _ = f.write_all(text.as_bytes());
                }
                writer.push(&text);
            },
        )
        .await;
        {
            let mut s = p.state.lock().unwrap_or_else(|e| e.into_inner());
            s.ended_at = Some(now_ms());
            match result {
                Ok(o) if o.cancelled => s.status = BackgroundStatus::Stopped,
                Ok(o) => {
                    s.status = BackgroundStatus::Exited;
                    s.exit_code = o.exit_code;
                }
                Err(e) => {
                    s.status = BackgroundStatus::Exited;
                    // 记进「新输出」，模型 read 时看得到为什么没起来。
                    let note = format!("\n[{e}]");
                    s.total += note.len();
                    s.tail.push_str(&note);
                }
            }
        }
        p.changed.notify_waiters();
        if let Some(f) = &on_update {
            f(&p.info());
        }
    });
    Ok(proc_)
}

pub fn get(id: &str) -> Option<Arc<BgProc>> {
    registry().lock().ok().and_then(|r| r.get(id).cloned())
}

/// 这个项目的后台命令（在跑的在前，最近的在前）；结束了的只留最近 20 个。
pub fn list(workspace: &Path) -> Vec<Arc<BgProc>> {
    let root = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
    let Ok(mut r) = registry().lock() else { return vec![] };
    let mut mine: Vec<Arc<BgProc>> = r.values().filter(|p| p.workspace == root).cloned().collect();
    mine.sort_by_key(|p| (!p.running(), -p.started_at));
    // 结束了的太多就忘掉旧的。
    for old in mine.iter().filter(|p| !p.running()).skip(20) {
        r.remove(&old.id);
    }
    mine.retain(|p| r.contains_key(&p.id));
    mine
}

/// 这个项目里叫这个名字、还在跑的常驻 server。
pub fn server(workspace: &Path, name: &str) -> Option<Arc<BgProc>> {
    list(workspace).into_iter().find(|p| p.kind == BackgroundKind::Server && p.name == name && p.running())
}

/// 任务结束：它起的后台命令都停掉（常驻的 server 不动）。
pub fn stop_owned(run_id: &str) {
    if let Ok(r) = registry().lock() {
        for p in r.values().filter(|p| p.owner.as_deref() == Some(run_id) && p.running()) {
            p.stop();
        }
    }
}

/// 退出 Peck：全停。
pub fn stop_all() {
    if let Ok(r) = registry().lock() {
        for p in r.values().filter(|p| p.running()) {
            p.stop();
        }
    }
}

pub fn event_of(info: &BgInfo) -> EventBody {
    EventBody::BackgroundUpdated {
        id: info.id.clone(),
        name: info.name.clone(),
        command: info.command.clone(),
        kind: info.kind,
        status: info.status,
        exit_code: info.exit_code,
    }
}

/// 给模型看的一行（环境块里）。
pub fn describe_for_model(p: &BgProc) -> String {
    let kind = match p.kind {
        BackgroundKind::Server => "dev server (resident; read only)",
        BackgroundKind::Command => "your background command",
    };
    let i = p.info();
    format!("{kind} \"{}\" `{}` id {} — {}", i.name, i.command, i.id, if i.ready { "running" } else { "starting" })
}

// ───────────────────────── IPC：常驻的 dev server ─────────────────────────

fn workspace_path(state: &crate::state::AppState, workspace_id: &str) -> Result<PathBuf, String> {
    workspace_of(state, workspace_id).map(|(root, _)| root)
}

/// 项目的文件夹和站主确认过的 verify.toml（md5）。
fn workspace_of(state: &crate::state::AppState, workspace_id: &str) -> Result<(PathBuf, Option<String>), String> {
    let db = state.store.lock().map_err(|_| "读不到项目。")?;
    let ws = db.workspace(workspace_id).map_err(|_| "没找到这个项目。")?;
    Ok((PathBuf::from(ws.path), ws.settings.verify_approved))
}

#[tauri::command]
pub fn background_list(workspace_id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<BgInfo>, String> {
    let root = workspace_path(&state, &workspace_id)?;
    Ok(list(&root).iter().map(|p| p.info()).collect())
}

/// 最近的输出（最多 64KB）。
#[tauri::command]
pub fn background_output(id: String) -> Result<String, String> {
    get(&id).map(|p| p.tail(64 * 1024)).ok_or_else(|| "这个后台命令不在了。".into())
}

#[tauri::command]
pub fn background_stop(id: String) -> Result<(), String> {
    get(&id).ok_or("这个后台命令不在了。")?.stop();
    Ok(())
}

/// 起 verify.toml 里的一个 server（已经在跑就返回它）。
#[tauri::command]
pub async fn server_start(workspace_id: String, name: String, state: tauri::State<'_, crate::state::AppState>) -> Result<BgInfo, String> {
    let (root, approved) = workspace_of(&state, &workspace_id)?;
    start_server(&root, &name, approved.as_deref(), None).map(|p| p.info())
}

/// 起常驻的 dev server（只认站主确认过的 verify.toml）。看在不在跑和起是一起的，站主点 DEV 和发任务时自动起不会起两份。
pub fn start_server(root: &Path, name: &str, approved: Option<&str>, on_update: Option<OnUpdate>) -> Result<Arc<BgProc>, String> {
    static STARTING: Mutex<()> = Mutex::new(());
    let _one = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = server(root, name) {
        return Ok(p);
    }
    let config = super::verify::load_approved(root, approved)?.ok_or("这个项目还没有 .peck/verify.toml。")?;
    let s = config.server.iter().find(|s| s.name == name).ok_or_else(|| format!("verify.toml 里没有叫「{name}」的 server。"))?;
    start(
        Spec {
            workspace: root.to_path_buf(),
            owner: None,
            name: s.name.clone(),
            command: s.run.clone(),
            cwd: None,
            kind: BackgroundKind::Server,
            ready: s.ready.clone(),
            sandbox: Sandbox::Project,
            env: bash::shell_env(),
            runtime_dir: super::run::runtime_dir(root),
            log_dir: super::run::spill_root().join(format!("servers-{}", crate::workspace::workspace_hash(root))),
        },
        on_update,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::tools::testing::TempProject;

    fn spec(p: &TempProject, command: &str, kind: BackgroundKind, owner: Option<&str>) -> Spec {
        Spec {
            workspace: p.dir.clone(),
            owner: owner.map(str::to_string),
            name: "t".into(),
            command: command.into(),
            cwd: None,
            kind,
            ready: Some("ready".into()),
            sandbox: Sandbox::Project,
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            runtime_dir: p.dir.with_extension("runtime"),
            log_dir: p.dir.with_extension("spill"),
        }
    }

    #[tokio::test]
    async fn runs_past_the_old_timeout_streams_and_stops_with_the_task() {
        let p = TempProject::new();
        let updates = Arc::new(Mutex::new(Vec::<BackgroundStatus>::new()));
        let u = updates.clone();
        let proc_ = start(
            spec(&p, "echo starting; sleep 0.3; echo ready; sleep 30", BackgroundKind::Command, Some("run-bg")),
            Some(Arc::new(move |i: &BgInfo| u.lock().unwrap().push(i.status))),
        )
        .unwrap();
        assert!(proc_.running() && !proc_.info().ready);
        for _ in 0..50 {
            if proc_.info().ready {
                break;
            }
            proc_.wait_change(Duration::from_millis(200)).await;
        }
        assert!(proc_.info().ready, "看到 ready 算起好了");
        let first = proc_.read_new();
        assert!(first.contains("starting") && first.contains("ready"));
        assert_eq!(proc_.read_new(), "", "读过的不再给");
        assert_eq!(list(&p.dir).len(), 1);
        stop_owned("run-bg");
        for _ in 0..50 {
            if !proc_.running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(proc_.info().status, BackgroundStatus::Stopped);
        assert_eq!(*updates.lock().unwrap(), vec![BackgroundStatus::Running, BackgroundStatus::Stopped]);
        assert!(std::fs::read_to_string(&proc_.log).unwrap().contains("ready"), "完整输出在日志里");
    }

    #[tokio::test]
    async fn servers_survive_the_task_and_exit_codes_are_kept() {
        let p = TempProject::new();
        let server = start(spec(&p, "sleep 30", BackgroundKind::Server, None), None).unwrap();
        stop_owned("any-run");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(server.running(), "常驻的不跟任务停");
        server.stop();
        let quick = start(spec(&p, "echo hi; exit 3", BackgroundKind::Command, Some("r")), None).unwrap();
        for _ in 0..100 {
            if !quick.running() && !server.running() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!((quick.info().status, quick.info().exit_code), (BackgroundStatus::Exited, Some(3)));
        assert!(quick.status_line().contains("退出码 3"));
    }
}
