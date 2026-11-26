//! `bash_run`：在沙箱里跑一条命令。
//!
//! - 沙箱（macOS 的 `sandbox-exec`）：**默认联网**；EDIT、AUTO 只能写项目、临时目录和 Peck 给的 HOME，
//!   `.git` 只读；PLAN 只能写临时目录。被沙箱拒了（写到项目外），循环会弹高危卡片问要不要出沙箱再跑。
//!   worker、member（v1.1.3）只能写划给它的那几块（`scope`），加上构建、缓存目录（[`BUILD_DIRS`]、git 忽略着的文件夹）。
//! - 环境：站主 shell 的那份（`term.rs` 读的），去掉名字像 Key、令牌、密码的变量和 `SSH_AUTH_SOCK`；stdin 接空，
//!   设 `CI=1`、`PAGER=cat`、`NO_COLOR=1`。
//! - 跑完（或超时、被停）就结束整个进程组，常驻进程不留。输出太长落盘（`spill.rs`）。
//! - 不是 macOS 的机器没有沙箱，直接跑；沙箱相关的测试只在 macOS 上跑。

use super::{str_arg, u64_arg, ToolContext};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};

pub const DEFAULT_TIMEOUT: u64 = 120;
pub const MAX_TIMEOUT: u64 = 600;
/// 内存里最多收这么多输出（再多的只数字节，不收）。
const KEEP_BYTES: usize = 32 * 1024 * 1024;
/// 发给界面的输出最多这么多（完整的落盘）。
const STREAM_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sandbox {
    /// EDIT、AUTO：能写项目、临时目录、Peck 给的 HOME。
    Project,
    /// PLAN：只能写临时目录。
    ReadOnly,
    /// AUTO 里只做提交的命令（v1.2.1，`risk::plain_commit`）：和 `Project` 一样，另外能写 `.git`（`config`、`hooks`、`info` 照样不能）。
    Commit,
    /// 站主在高危卡片上批了「出沙箱再跑」。
    Off,
}

#[derive(Debug, Clone)]
pub struct BashOutcome {
    pub command: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    /// stdout 和 stderr 按到达顺序合在一起。
    pub output: String,
    pub duration: Duration,
    pub sandbox: Sandbox,
}

impl BashOutcome {
    /// 算不算成功：退出码 0；grep、rg、diff、cmp 的退出码 1（没找到、有差异）也算。
    pub fn ok(&self) -> bool {
        match self.exit_code {
            Some(0) => true,
            Some(1) => soft_exit_one(&self.command),
            _ => false,
        }
    }

    /// 看起来是被沙箱拦了（写到不让写的地方）。
    pub fn sandbox_denied(&self) -> bool {
        self.sandbox != Sandbox::Off
            && cfg!(target_os = "macos")
            && !self.ok()
            && !self.timed_out
            && !self.cancelled
            && (self.output.contains("Operation not permitted") || self.output.contains("Read-only file system"))
    }

    /// 被挡的是写 `.git`（提交、切分支这类，v1.1.12）：拒绝的那一行里提到 `.git/`（`.github/` 不算）。卡片原因照这个写，不再一律写「项目外」。
    pub fn denied_git(&self) -> bool {
        self.output.lines().any(|l| (l.contains("Operation not permitted") || l.contains("Read-only file system")) && l.contains(".git/"))
    }

    pub fn status_line(&self) -> String {
        let secs = format!("{:.1}s", self.duration.as_secs_f64());
        if self.cancelled {
            format!("已停止（{secs}）")
        } else if self.timed_out {
            format!("超时了（{secs}），整个进程组已结束")
        } else {
            match self.exit_code {
                Some(code) if self.ok() && code != 0 => format!("退出码 {code}（{secs}，这个命令的 1 不算失败）"),
                Some(code) => format!("退出码 {code}（{secs}）"),
                None => format!("被信号结束（{secs}）"),
            }
        }
    }
}

/// 最后一段命令是 grep、rg、diff、cmp 这类：退出码 1 不是出错。
pub fn soft_exit_one(command: &str) -> bool {
    let last = command
        .rsplit(['|', ';', '\n'])
        .next()
        .unwrap_or(command)
        .rsplit("&&")
        .next()
        .unwrap_or("");
    let first = last.split_whitespace().find(|w| !w.contains('=')).unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or(first);
    matches!(name, "grep" | "egrep" | "fgrep" | "rg" | "ag" | "diff" | "cmp" | "colordiff")
        || (name == "git" && last.split_whitespace().any(|w| w == "diff"))
}

/// 名字像 Key、令牌、密码的变量不带进命令。`SSH_AUTH_SOCK` 也去掉（站主 2026-11-13 定）：
/// 命令用不了 ssh-agent 里的钥匙（经 SSH 推送多一道挡；磁盘上没设口令的钥匙文件不在此列）。
pub fn looks_secret(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    if n == "SSH_AUTH_SOCK" || n == "SSH_AGENT_PID" {
        return true;
    }
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "_AUTH", "AUTH_", "COOKIE", "SESSION", "PRIVATE", "DATABASE_URL"]
        .iter()
        .any(|w| n.contains(w))
        // 只看结尾，免得把 `*_PATH` 这类也去掉
        || ["_PASS", "_PAT", "_DSN", "_PWD"].iter().any(|w| n.ends_with(w))
}

/// 跑命令用的环境：站主 shell 的那份（读不到就用 App 自己的），去掉像密钥的变量。
pub fn shell_env() -> Vec<(String, String)> {
    let base: Vec<(String, String)> = match crate::term::user_env() {
        Some(vars) => vars.clone(),
        None => {
            let mut vars: Vec<(String, String)> = std::env::vars().collect();
            let path = crate::agent::build_augmented_path(&[]);
            vars.retain(|(k, _)| k != "PATH");
            vars.push(("PATH".into(), path));
            vars
        }
    };
    base.into_iter().filter(|(k, _)| !looks_secret(k) && !k.starts_with("PECK_")).collect()
}

fn sb(path: &Path) -> String {
    serde_json::Value::String(path.to_string_lossy().into_owned()).to_string()
}

/// 这个文件夹下面任何一层的 `.git`（比如 `~/code` 里各个仓库的钩子）：SBPL 的正则。路径里有引号就不写这条。
fn nested_git(dir: &Path) -> Option<String> {
    let s = dir.to_string_lossy();
    if s.contains('"') {
        return None;
    }
    let mut out = String::from("(regex #\"^");
    for c in s.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push_str("/(.+/)?\\.git(/|$)\")");
    Some(out)
}

/// 构建、缓存目录（v1.1.3）：划了文件的 worker、member 跑构建、测试时也能写（不算改乱：都是生成的）。
pub const BUILD_DIRS: [&str; 22] = [
    "target", "node_modules", "dist", "build", "out", ".next", ".nuxt", ".svelte-kit", ".turbo", ".vite", ".cache", ".parcel-cache",
    "coverage", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".venv", "venv", ".gradle", "DerivedData", ".tox",
];

/// 项目下面任何一层的构建、缓存目录，和 `*.tsbuildinfo`、`.eslintcache`：SBPL 的正则。路径里有引号就不写。
fn build_dirs(root: &Path) -> Option<String> {
    let s = root.to_string_lossy();
    if s.contains('"') {
        return None;
    }
    let names: Vec<String> = BUILD_DIRS.iter().map(|n| n.replace('.', "\\.")).collect();
    Some(format!("(regex #\"^{}/((.+/)?({})(/|$)|(.+/)?[^/]*\\.tsbuildinfo$|(.+/)?\\.eslintcache$)\")", escape_regex(&s), names.join("|")))
}

fn escape_regex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 沙箱规则（Apple 的 SBPL，后面的规则优先）。默认全放开（读、联网、起进程），只管写；
/// 另外不让开 App（`open -a Terminal x.command`）、不让发 Apple Events（`osascript` 叫别的 App 跑），
/// 这两条路跑起来的东西在沙箱外、也不在进程组里。
/// `scope`（v1.1.3，worker、member）：项目里只放开这几块（绝对路径）和构建、缓存目录，不是整个项目。
pub fn sandbox_profile(root: &Path, temp: &Path, home: &Path, sandbox: Sandbox, extra_writable: &[PathBuf], extra_read_only: &[PathBuf], scope: Option<&[PathBuf]>) -> String {
    let mut writable = vec![format!("(subpath {})", sb(temp)), "(subpath \"/dev\")".to_string()];
    let mut git = vec![format!("(subpath {})", sb(&root.join(".git")))];
    git.extend(nested_git(root));
    // 只读的其他文件夹：明着不让写（它在临时目录这类本来能写的地方时也挡住）。
    git.extend(extra_read_only.iter().map(|p| format!("(subpath {})", sb(p))));
    let mut commit = Vec::new();
    if matches!(sandbox, Sandbox::Project | Sandbox::Commit) {
        if sandbox == Sandbox::Commit {
            for dir in git_dirs(root).into_iter().chain(extra_writable.iter().flat_map(|e| git_dirs(e))) {
                commit.push(format!("(allow file-write* (subpath {}))", sb(&dir)));
                commit.push(format!("(deny file-write* (literal {}) (literal {}) (subpath {}) (subpath {}))", sb(&dir.join("config")), sb(&dir.join("config.worktree")), sb(&dir.join("hooks")), sb(&dir.join("info"))));
            }
        }
        match scope {
            Some(parts) => {
                writable.extend(parts.iter().map(|p| format!("(subpath {})", sb(p))));
                writable.extend(build_dirs(root));
            }
            None => writable.push(format!("(subpath {})", sb(root))),
        }
        writable.push(format!("(subpath {})", sb(home)));
        writable.push("(subpath \"/private/tmp\")".into());
        writable.push("(subpath \"/private/var/folders\")".into());
        // 项目里「能改」的其他文件夹（v0.7）：一样能写，它们的 .git 一样只读（划了文件的只能写划给它的）。
        for extra in extra_writable {
            if scope.is_none() {
                writable.push(format!("(subpath {})", sb(extra)));
            }
            git.push(format!("(subpath {})", sb(&extra.join(".git"))));
            git.extend(nested_git(extra));
        }
    }
    format!(
        "(version 1)(allow default)\
         (deny file-write*)\
         (allow file-write* {})\
         (deny file-write* {})\
         {}\
         {KEYCHAIN}\
         (deny lsopen)\
         (deny appleevent-send)",
        writable.join(" "),
        git.join(" "),
        commit.join(""),
    )
}

/// 这个文件夹的 git 目录：`.git` 是文件夹就是它；是文件（git worktree）就是它指的那个，加上共用的那个（`commondir`）。
fn git_dirs(dir: &Path) -> Vec<PathBuf> {
    let dot = dir.join(".git");
    if dot.is_dir() {
        return dot.canonicalize().into_iter().collect();
    }
    let Some(gitdir) = std::fs::read_to_string(&dot).ok().and_then(|t| t.trim().strip_prefix("gitdir:").map(|g| dir.join(g.trim()))) else { return vec![] };
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok().map(|c| gitdir.join(c.trim()));
    [Some(gitdir), common].into_iter().flatten().filter_map(|d| d.canonicalize().ok()).collect()
}

/// Peck 给沙箱里的命令设的记号（v1.1.12，照 Codex 的 `CODEX_SANDBOX`）：macOS 不让沙箱里的进程再套一层沙箱，
/// Peck 自己的测试看到它就跳过要套沙箱的那几个（2026-11-25 PLOOP 开发 Peck 时全量测试在沙箱里卡住、一直没跑完）。
pub const SANDBOX_ENV: &str = "PECK_SANDBOX";

/// 测试用：在 Peck 的沙箱里跑（套不了第二层沙箱）就说一声、返回 true，调用的测试直接结束。
#[cfg(test)]
pub(crate) fn nested_sandbox() -> bool {
    let inside = std::env::var_os(SANDBOX_ENV).is_some();
    if inside {
        eprintln!("跳过：在 Peck 的沙箱里，套不了第二层沙箱（在外面跑全量测试才验得到）");
    }
    inside
}

/// cargo 的下载缓存（v1.1.12）：EDIT、AUTO 的命令用站主自己的 `~/.cargo`，只放开它的下载缓存和锁（`bin`、配置照样不能写）。
/// 以前给的是 Peck 那个 HOME 里的 `.cargo`：依赖的源码路径和站主平时的不一样，cargo 每换一边就把全部依赖重编一遍
/// （2026-11-25 PLOOP 开发 Peck 时一次全量测试跑了 12 分钟还没出结果）。路径里有引号就不放开。
pub fn cargo_caches(cargo_home: &Path) -> Option<String> {
    let s = cargo_home.to_string_lossy();
    if s.contains('"') {
        return None;
    }
    Some(format!(
        "(allow file-write* (subpath {}) (subpath {}) (regex #\"^{}/\\.(package-cache|package-cache-mutate|global-cache)\"))",
        sb(&cargo_home.join("registry")),
        sb(&cargo_home.join("git")),
        escape_regex(&s),
    ))
}

/// 钥匙串的几个服务（v1.1.11）：命令碰不到钥匙串。`com.apple.SecurityServer` 是老的文件钥匙串，命令的 HOME 是 Peck 给的，
/// 里面没有钥匙串，不挡的话 Chrome 这类程序一起来系统就连弹「Keychain Not Found」（2026-11-25 PLOOP 起无头 Chrome 时弹了一串）。
pub(super) const KEYCHAIN: &str = "(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.securityd\") (global-name \"com.apple.securityd.xpc\"))";

/// 出沙箱跑（站主批了）也照样挡钥匙串。
fn keychain_only_profile() -> String {
    format!("(version 1)(allow default){KEYCHAIN}")
}

/// 增量的输出：按 UTF-8 切好，不切坏多字节字符。
struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        match std::str::from_utf8(&self.pending) {
            Ok(s) => {
                let out = s.to_string();
                self.pending.clear();
                out
            }
            Err(e) if e.error_len().is_none() => {
                let valid = e.valid_up_to();
                let out = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
                self.pending.drain(..valid);
                out
            }
            Err(_) => {
                let out = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                out
            }
        }
    }
}

/// 跑。`on_output` 收增量（攒一会儿再给，免得一个字节一条事件）；`cancel` 变成 true 就停。
pub async fn run(
    ctx: &ToolContext,
    input: &serde_json::Value,
    sandbox: Sandbox,
    scope: Option<&[PathBuf]>,
    cancel: watch::Receiver<bool>,
    on_output: impl FnMut(String),
) -> Result<BashOutcome, String> {
    let command = str_arg(input, "command").map(str::trim).filter(|c| !c.is_empty()).ok_or("缺 command。")?.to_string();
    let cwd = ctx.resolve_path(str_arg(input, "cwd").unwrap_or("."), false)?;
    let timeout = Duration::from_secs(u64_arg(input, "timeout_sec").unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT));
    run_command(ctx, Exec { command: &command, cwd: &cwd, timeout, sandbox, scope, keep_bytes: KEEP_BYTES, stream_bytes: STREAM_BYTES }, cancel, on_output).await
}

/// 一次命令怎么跑：`bash_run` 是 120 秒、收进内存；后台命令（v0.6）不限时、只往外流（Peck 自己存）。
pub struct Exec<'a> {
    pub command: &'a str,
    /// 已经解析过、在项目里的绝对路径。
    pub cwd: &'a Path,
    pub timeout: Duration,
    pub sandbox: Sandbox,
    /// 划了文件的 worker、member（v1.1.3）：沙箱只放开这几块（绝对路径），不是整个项目。
    pub scope: Option<&'a [PathBuf]>,
    /// 内存里最多收多少（`BashOutcome::output`）。
    pub keep_bytes: usize,
    /// 经 `on_output` 往外流的最多多少。
    pub stream_bytes: usize,
}

/// 跑一条命令（`bash_run`、后台命令共用）：沙箱、环境、进程组都一样。
pub async fn run_command(
    ctx: &ToolContext,
    exec: Exec<'_>,
    mut cancel: watch::Receiver<bool>,
    mut on_output: impl FnMut(String),
) -> Result<BashOutcome, String> {
    let Exec { command, cwd, timeout, sandbox, scope, keep_bytes, stream_bytes } = exec;
    let command = command.to_string();
    if !cwd.is_dir() {
        return Err(format!("{} 不是文件夹。", ctx.rel(cwd)));
    }
    std::fs::create_dir_all(ctx.runtime_dir.join("home")).map_err(|e| format!("建不了命令的 HOME：{e}"))?;
    let runtime = ctx.runtime_dir.canonicalize().map_err(|e| e.to_string())?;
    let home = runtime.join("home");
    let temp = runtime.join(format!("tmp-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;

    let real_home = ctx.env.iter().find(|(k, _)| k == "HOME").map(|(_, v)| PathBuf::from(v));
    // 站主有 `~/.cargo` 就用它（见 `cargo_caches`）；PLAN 写不了缓存，照旧用 Peck 给的。
    let real_cargo = real_home.as_ref().map(|h| h.join(".cargo")).filter(|c| sandbox != Sandbox::ReadOnly && c.is_dir());
    let cargo_home = real_cargo.clone().unwrap_or_else(|| home.join(".cargo"));
    let mut cmd = if cfg!(target_os = "macos") {
        let mut profile = if sandbox == Sandbox::Off { keychain_only_profile() } else { sandbox_profile(&ctx.root, &temp, &home, sandbox, &ctx.writable_extra(), &ctx.read_only_extra(), scope) };
        if matches!(sandbox, Sandbox::Project | Sandbox::Commit) {
            profile.push_str(&real_cargo.as_deref().and_then(cargo_caches).unwrap_or_default());
        }
        let mut c = tokio::process::Command::new("/usr/bin/sandbox-exec");
        c.args(["-p", &profile, "/bin/sh", "-c", &command]);
        c
    } else {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.args(["-c", &command]);
        c
    };
    cmd.current_dir(cwd)
        .env_clear()
        .envs(ctx.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .env("HOME", &home)
        .env("TMPDIR", &temp)
        .env("CI", "1")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .env("CARGO_HOME", &cargo_home)
        .env("COREPACK_ENABLE_AUTO_PIN", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if sandbox != Sandbox::Off {
        cmd.env(SANDBOX_ENV, "seatbelt");
    }
    // git 的全局配置（v1.1.12）：HOME 是 Peck 给的，git 读不到站主的 user.name、user.email，提交就失败（PLOOP 开发
    // Peck 时只好自己在命令里带 GIT_CONFIG_GLOBAL）。指到站主自己的那份，沙箱里只能读、改不了。
    if !ctx.env.iter().any(|(k, _)| k == "GIT_CONFIG_GLOBAL") {
        if let Some(global) = real_home.as_ref().and_then(|h| [h.join(".gitconfig"), h.join(".config/git/config")].into_iter().find(|p| p.is_file())) {
            cmd.env("GIT_CONFIG_GLOBAL", global);
        }
    }
    if let Some(real) = &real_home {
        if real.join(".rustup").is_dir() {
            cmd.env("RUSTUP_HOME", real.join(".rustup"));
        }
    }
    #[cfg(unix)]
    cmd.process_group(0);
    if *cancel.borrow() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err("已停止，命令没起。".into());
    }
    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("命令起不来：{e}"))?;
    let pgid = child.id();
    let mut group = crate::process::GroupGuard::new(pgid);
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    if let Some(out) = child.stdout.take() {
        pump(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump(err, tx.clone());
    }
    drop(tx);

    let mut kept: Vec<u8> = Vec::new();
    let mut dropped = 0usize;
    let mut streamed = 0usize;
    let mut decoder = Utf8Stream { pending: Vec::new() };
    let mut pending = String::new();
    let mut last_flush = Instant::now();
    let mut take = |bytes: Vec<u8>, kept: &mut Vec<u8>, dropped: &mut usize, pending: &mut String, streamed: &mut usize| {
        if kept.len() < keep_bytes {
            let room = keep_bytes - kept.len();
            kept.extend_from_slice(&bytes[..bytes.len().min(room)]);
            *dropped += bytes.len().saturating_sub(room);
        } else {
            *dropped += bytes.len();
        }
        if *streamed < stream_bytes {
            let text = decoder.push(&bytes);
            *streamed += text.len();
            pending.push_str(&text);
        }
    };
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut exit: Option<std::process::ExitStatus> = None;
    let (mut timed_out, mut cancelled) = (false, false);
    let mut pipes_open = true;
    loop {
        if !pending.is_empty() && (pending.len() > 8 * 1024 || last_flush.elapsed() > Duration::from_millis(120)) {
            on_output(std::mem::take(&mut pending));
            last_flush = Instant::now();
        }
        if exit.is_some() && !pipes_open {
            break;
        }
        tokio::select! {
            biased;
            // 攒着的输出没人催也按时送出去（安静的常驻进程，第一行不能等到它退出）。
            _ = tokio::time::sleep(Duration::from_millis(120)), if !pending.is_empty() => {}
            changed = cancel.changed(), if !cancelled => {
                if changed.is_err() || *cancel.borrow() {
                    cancelled = true;
                    break;
                }
            }
            _ = &mut deadline, if exit.is_none() => { timed_out = true; break; }
            chunk = rx.recv(), if pipes_open => match chunk {
                Some(bytes) => take(bytes, &mut kept, &mut dropped, &mut pending, &mut streamed),
                None => pipes_open = false,
            },
            status = child.wait(), if exit.is_none() => {
                exit = status.ok();
                // 进程退出了：后台的子进程不留，剩下的输出再收一小会儿。
                crate::process::terminate_process_group(pgid).await;
                let grace = tokio::time::sleep(Duration::from_millis(500));
                tokio::pin!(grace);
                loop {
                    tokio::select! {
                        chunk = rx.recv() => match chunk {
                            Some(bytes) => take(bytes, &mut kept, &mut dropped, &mut pending, &mut streamed),
                            None => { pipes_open = false; break; }
                        },
                        _ = &mut grace => { pipes_open = false; break; }
                    }
                }
            }
        }
    }
    crate::process::terminate_process_group(pgid).await;
    group.disarm();
    let _ = child.kill().await;
    if exit.is_none() {
        exit = child.wait().await.ok();
    }
    while let Ok(bytes) = rx.try_recv() {
        take(bytes, &mut kept, &mut dropped, &mut pending, &mut streamed);
    }
    if !pending.is_empty() {
        on_output(pending);
    }
    let _ = std::fs::remove_dir_all(&temp);
    let mut output = String::from_utf8_lossy(&kept).into_owned();
    if dropped > 0 {
        output.push_str(&format!("\n[还有 {} KB 输出太多，没收下]", dropped / 1024));
    }
    Ok(BashOutcome {
        command,
        exit_code: if timed_out || cancelled { None } else { exit.and_then(|s| s.code()) },
        timed_out,
        cancelled,
        output,
        duration: started.elapsed(),
        sandbox,
    })
}

/// 把一个管道的输出一段段送进 `tx`（stdout、stderr 合在一起，按到达顺序）。
fn pump<R: tokio::io::AsyncRead + Unpin + Send + 'static>(mut reader: R, tx: mpsc::UnboundedSender<Vec<u8>>) {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::testing::TempProject;
    use super::*;
    use serde_json::json;

    fn never() -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        std::mem::forget(tx);
        rx
    }

    #[test]
    fn grep_and_diff_exit_one_is_not_a_failure() {
        assert!(soft_exit_one("grep -n foo src/a.ts"));
        assert!(soft_exit_one("cat a | rg x"));
        assert!(soft_exit_one("cd src && diff a b"));
        assert!(soft_exit_one("git diff --exit-code"));
        assert!(!soft_exit_one("npm test"));
        assert!(!soft_exit_one("grep x a && npm test"));
    }

    #[test]
    fn secrets_stay_out_of_the_environment() {
        for k in ["OPENAI_API_KEY", "GITHUB_TOKEN", "AWS_SECRET_ACCESS_KEY", "DB_PASSWORD", "NPM_AUTH_TOKEN", "SMTP_PASS", "GH_PAT", "SENTRY_DSN", "DATABASE_URL", "MYSQL_PWD", "SSH_AUTH_SOCK", "SSH_AGENT_PID"] {
            assert!(looks_secret(k), "{k}");
        }
        for k in ["PATH", "HOME", "LANG", "NVM_DIR", "EDITOR", "DYLD_FALLBACK_LIBRARY_PATH", "MANPATH", "PWD", "OLDPWD"] {
            assert!(!looks_secret(k), "{k}");
        }
    }

    #[test]
    fn profile_allows_network_and_limits_writes() {
        let root = Path::new("/Users/x/proj");
        let p = sandbox_profile(root, Path::new("/t/tmp-1"), Path::new("/t/home"), Sandbox::Project, &[PathBuf::from("/other/lib")], &[PathBuf::from("/private/tmp/ro")], None);
        assert!(p.contains("(allow default)") && !p.contains("deny network"));
        assert!(p.contains("(subpath \"/Users/x/proj\")") && p.contains("(subpath \"/t/home\")"));
        assert!(p.contains("(deny file-write* (subpath \"/Users/x/proj/.git\")"));
        assert!(p.contains("(subpath \"/other/lib/.git\")") && p.contains(r#"(regex #"^/other/lib/(.+/)?\.git(/|$)")"#), "{p}");
        assert!(p.contains(r#"(regex #"^/Users/x/proj/(.+/)?\.git(/|$)")"#));
        let deny = &p[p.rfind("(deny file-write*").unwrap()..];
        assert!(deny.contains("(subpath \"/private/tmp/ro\")"), "只读的其他文件夹明着不让写");
        assert!(p.contains("(subpath \"/other/lib\")"), "能改的其他文件夹也能写");
        let ro = sandbox_profile(root, Path::new("/t/tmp-1"), Path::new("/t/home"), Sandbox::ReadOnly, &[PathBuf::from("/other/lib")], &[], None);
        assert!(!ro.contains("(subpath \"/Users/x/proj\")") && ro.contains("/t/tmp-1"));
        assert!(!ro.contains("(subpath \"/other/lib\")"), "PLAN 里其他文件夹也不能写");
        // 钥匙串（v1.1.11）：老的文件钥匙串服务也挡，出沙箱的那份也挡。
        for profile in [&p, &ro, &keychain_only_profile()] {
            assert!(profile.contains("com.apple.SecurityServer") && profile.contains("com.apple.securityd.xpc"), "{profile}");
        }
    }

    /// 真起一次：挡了钥匙串服务以后 `security` 连钥匙串列表都拿不到（连得上时至少列出系统钥匙串）。只读查询，不弹框。
    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn keychain_is_out_of_reach() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        let ctx = p.ctx();
        for sandbox in [Sandbox::Project, Sandbox::Off] {
            let out = run(&ctx, &json!({"command": "security list-keychains 2>&1"}), sandbox, None, watch::channel(false).1, |_| {}).await.unwrap();
            assert!(!out.output.contains("System.keychain"), "{sandbox:?}: 钥匙串还连得上：{}", out.output);
        }
    }

    /// AUTO 的提交（v1.2.1）：`Commit` 能 `git add`、`git commit`，`.git/config`、`hooks` 照样写不了；`Project` 写不了 `.git`。
    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn the_commit_sandbox_writes_git_but_not_its_config_or_hooks() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        p.write("a.txt", "a\n");
        let ctx = p.ctx();
        let git = |c: &str| format!("git -c user.name=t -c user.email=t@e {c}");
        assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(&p.dir).status().unwrap().success());
        let commit = format!("{} && {}", git("add -A"), git("commit -qm 'v1.0.0: x'"));
        let blocked = run(&ctx, &json!({ "command": commit }), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(blocked.sandbox_denied() && blocked.denied_git(), "{}", blocked.output);
        let done = run(&ctx, &json!({ "command": commit }), Sandbox::Commit, None, never(), |_| {}).await.unwrap();
        assert!(done.ok(), "{}", done.output);
        for write in ["echo x >> .git/config", "echo x > .git/hooks/pre-commit", "echo x > .git/info/exclude"] {
            let out = run(&ctx, &json!({ "command": write }), Sandbox::Commit, None, never(), |_| {}).await.unwrap();
            assert!(out.sandbox_denied(), "{write}: {}", out.output);
        }
    }

    #[tokio::test]
    async fn runs_streams_and_reports_exit_codes() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        p.write("a.txt", "hello\n");
        let ctx = p.ctx();
        let mut seen = String::new();
        let out = run(&ctx, &json!({"command":"cat a.txt; echo err >&2; exit 3"}), Sandbox::Project, None, never(), |s| seen.push_str(&s)).await.unwrap();
        assert_eq!(out.exit_code, Some(3));
        assert!(!out.ok());
        assert!(out.output.contains("hello") && out.output.contains("err"));
        assert!(seen.contains("hello"));
        let env = run(&ctx, &json!({"command":"echo $CI $PAGER $NO_COLOR; test -t 0 || echo nostdin"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(env.output.contains("1 cat 1") && env.output.contains("nostdin"), "{}", env.output);
        let grep = run(&ctx, &json!({"command":"grep nothing a.txt"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert_eq!(grep.exit_code, Some(1));
        assert!(grep.ok());
        assert!(run(&ctx, &json!({"command":"ls","cwd":"../"}), Sandbox::Project, None, never(), |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn timeouts_and_stops_end_the_whole_group() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        let ctx = p.ctx();
        let started = Instant::now();
        let out = run(&ctx, &json!({"command":"(sleep 30; echo late > late.txt) & sleep 30","timeout_sec":1}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(out.timed_out && started.elapsed() < Duration::from_secs(10));
        let (tx, rx) = watch::channel(false);
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = tx.send(true);
            tx
        });
        let out = run(&ctx, &json!({"command":"sleep 30"}), Sandbox::Project, None, rx, |_| {}).await.unwrap();
        assert!(out.cancelled);
        drop(stopper.await);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!p.dir.join("late.txt").exists(), "后台子进程也结束了");
        // 进程退出后留下的后台进程不会让它一直等
        let started = Instant::now();
        let out = run(&ctx, &json!({"command":"sleep 30 & echo started"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn dropping_the_command_future_ends_the_whole_group() {
        // 调用方直接丢掉 future（abort）时：孙进程也不能留下。
        let p = TempProject::new();
        let ctx = p.ctx();
        let task = tokio::spawn(async move {
            run(&ctx, &json!({"command":"(sleep 2; echo late > late.txt) & sleep 30"}), Sandbox::Project, None, never(), |_| {}).await
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!p.dir.join("late.txt").exists(), "后台子进程跟着结束了");
    }

    /// 划了文件的 worker、member（v1.1.3）：只写得了划给它的那几块和构建、缓存目录，不是整个项目。
    /// （测试项目在临时目录里，临时目录本来整个能写，所以「写不了别处」看规则本身。）
    #[test]
    fn a_scoped_profile_opens_only_its_paths_and_build_dirs() {
        let root = Path::new("/Users/x/proj");
        let scoped = sandbox_profile(root, Path::new("/t/tmp-1"), Path::new("/t/home"), Sandbox::Project, &[PathBuf::from("/other/lib")], &[], Some(&[root.join("src/a")]));
        let allow = &scoped[..scoped.find("(deny file-write* (subpath").unwrap()];
        assert!(allow.contains("(subpath \"/Users/x/proj/src/a\")") && !allow.contains("(subpath \"/Users/x/proj\")"), "{allow}");
        assert!(allow.contains("node_modules") && allow.contains("tsbuildinfo") && !allow.contains("/other/lib"), "{allow}");
        assert!(scoped.contains("(subpath \"/Users/x/proj/.git\")"), ".git 照样只读");
        let whole = sandbox_profile(root, Path::new("/t/tmp-1"), Path::new("/t/home"), Sandbox::Project, &[], &[], None);
        assert!(whole.contains("(subpath \"/Users/x/proj\")") && !whole.contains("node_modules"));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_scoped_sandbox_still_runs_builds() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        p.write("mine/a.txt", "x");
        let ctx = p.ctx();
        let scope = vec![ctx.root.join("mine")];
        let ok = run(&ctx, &json!({"command":"echo z > mine/b.txt && mkdir -p app/node_modules/.cache && echo o > app/node_modules/.cache/x && echo t > tsconfig.tsbuildinfo"}), Sandbox::Project, Some(&scope), never(), |_| {}).await.unwrap();
        assert!(ok.ok(), "{}", ok.output);
        assert_eq!(p.read("mine/b.txt"), "z\n");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_sandbox_blocks_writes_outside_but_not_the_network() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        let ctx = p.ctx();
        let outside = std::env::var("HOME").unwrap() + "/peck-sandbox-probe.txt";
        let out = run(&ctx, &json!({"command": format!("echo x > {outside}")}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(!out.ok() && out.sandbox_denied(), "{}", out.output);
        assert!(!Path::new(&outside).exists());
        let inside = run(&ctx, &json!({"command":"echo x > ok.txt"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(inside.ok());
        let ro = run(&ctx, &json!({"command":"echo x > ro.txt"}), Sandbox::ReadOnly, None, never(), |_| {}).await.unwrap();
        assert!(!ro.ok() && !p.dir.join("ro.txt").exists());
        std::fs::create_dir_all(p.dir.join(".git")).unwrap();
        let git = run(&ctx, &json!({"command":"echo x > .git/x"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(!git.ok() && git.denied_git(), "{}", git.output);
        assert!(!out.denied_git(), "写项目外的不算 .git");
        let net = run(&ctx, &json!({"command":"/usr/bin/curl -sS -o /dev/null -w '%{http_code}' https://example.com || true"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(!net.output.contains("Operation not permitted"), "{}", net.output);
    }

    /// 站主自己的 `~/.cargo`、`~/.gitconfig`（v1.1.12）：EDIT、AUTO 的命令用它们（git 认得出是谁提交），PLAN 不用 cargo 的。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn commands_use_the_users_cargo_home_and_git_identity() {
        if nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        let home = p.dir.with_extension("home");
        std::fs::create_dir_all(home.join(".cargo")).unwrap();
        std::fs::write(home.join(".gitconfig"), "[user]\n\tname = Peck Test\n").unwrap();
        let mut ctx = p.ctx();
        ctx.env.push(("HOME".into(), home.to_string_lossy().into_owned()));
        let cmd = json!({"command": "echo \"$CARGO_HOME\"; git config --global user.name"});
        let out = run(&ctx, &cmd, Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(out.ok(), "{}", out.output);
        let mut lines = out.output.lines();
        assert_eq!(lines.next().map(PathBuf::from), Some(home.join(".cargo")));
        assert_eq!(lines.next(), Some("Peck Test"));
        let ro = run(&ctx, &cmd, Sandbox::ReadOnly, None, never(), |_| {}).await.unwrap();
        assert!(!ro.output.lines().next().unwrap_or("").starts_with(&*home.to_string_lossy()), "PLAN 照旧用 Peck 给的：{}", ro.output);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// cargo 的下载缓存（v1.1.12）：只放开 registry、git 和那几个锁，`bin`、配置照样写不了。
    #[cfg(target_os = "macos")]
    #[test]
    fn cargo_caches_open_only_the_download_cache() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let cargo = std::env::temp_dir().canonicalize().unwrap().join(format!("peck-cargo-{}", uuid::Uuid::new_v4())).join(".cargo");
        for d in ["registry/src", "git", "bin"] {
            std::fs::create_dir_all(cargo.join(d)).unwrap();
        }
        let profile = format!("(version 1)(allow default)(deny file-write*){}", cargo_caches(&cargo).unwrap());
        let sh = |cmd: &str| std::process::Command::new("/usr/bin/sandbox-exec").args(["-p", &profile, "/bin/sh", "-c", cmd]).current_dir(&cargo).status().unwrap().success();
        assert!(sh("echo x > registry/src/a && echo x > git/b && echo x > .package-cache && echo x > .global-cache-journal"));
        assert!(!sh("echo x > bin/evil") && !cargo.join("bin/evil").exists(), "bin 不能写");
        assert!(!sh("echo x > config.toml") && !cargo.join("config.toml").exists(), "配置不能写");
        assert!(cargo_caches(Path::new("/a\"b")).is_none());
        let _ = std::fs::remove_dir_all(cargo.parent().unwrap());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_sandbox_guards_nested_git_and_read_only_folders() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let p = TempProject::new();
        // 只读的其他文件夹放在 /private/tmp 下（那里本来谁都能写）
        let ro = std::path::PathBuf::from(format!("/private/tmp/peck-ro-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ro).unwrap();
        let ctx = p.ctx().with_extra(&[(ro.clone(), false)]);
        std::fs::create_dir_all(p.dir.join("vendor/sub/.git/hooks")).unwrap();
        std::fs::create_dir_all(p.dir.join(".github")).unwrap();
        let hook = run(&ctx, &json!({"command":"echo x > vendor/sub/.git/hooks/post-checkout"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(!hook.ok() && !p.dir.join("vendor/sub/.git/hooks/post-checkout").exists(), "{}", hook.output);
        let gh = run(&ctx, &json!({"command":"echo x > .github/ok.yml && echo x > vendor/sub/ok.txt"}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(gh.ok(), "{}", gh.output);
        let write_ro = run(&ctx, &json!({"command": format!("echo x > {}/x.txt", ro.display())}), Sandbox::Project, None, never(), |_| {}).await.unwrap();
        assert!(!write_ro.ok() && !ro.join("x.txt").exists(), "{}", write_ro.output);
        let _ = std::fs::remove_dir_all(&ro);
    }
}
