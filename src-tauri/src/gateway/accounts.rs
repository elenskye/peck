//! CLI 账号：一个账号 = 一个独立的配置目录（Claude Code 用 `CLAUDE_CONFIG_DIR`，Codex 用 `CODEX_HOME`）。
//! 不同账号的登录互不干扰，也不碰 Claude 客户端、ChatGPT 客户端和浏览器里的登录。
//!
//! - 每个引擎都有一个「默认」账号：CLI 自己的默认位置（`~/.claude`、`~/.codex`），和客户端共用。
//! - 新增的账号放在「CLI 目录」旁边的 `accounts/<引擎>/<名字>/`（`cli/bin` → `cli/accounts/…`），
//!   没设 CLI 目录就放 Peck 数据目录的 `accounts/`。
//! - 登录在伪终端里跑 CLI 自己的登录命令；不自动开浏览器（`open`、`BROWSER` 换成只记下链接的小脚本），
//!   设置里显示链接，站主自己挑浏览器或隐身窗口去登。Codex 用 `--device-auth`（链接 + 一次性代码）。
//! - Peck 只设环境变量、读状态命令的输出；不读、不复用任何凭据。
//!
//! 记在 `app_settings` 的 `cli_accounts` 键：每个引擎当前用哪个、新增过哪些。

use super::config::GatewayConfig;
use crate::store::Store;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const KEY: &str = "cli_accounts";
pub const DEFAULT_ID: &str = "default";

/// 支持多账号的引擎：(引擎 id, 显示名, 命令, 配置目录的环境变量)。
const ENGINES: [(&str, &str, &str, &str); 2] = [
    ("claude-code", "Claude Code", "claude", "CLAUDE_CONFIG_DIR"),
    ("codex", "Codex", "codex", "CODEX_HOME"),
];

fn spec(engine: &str) -> Result<(&'static str, &'static str, &'static str, &'static str), String> {
    ENGINES
        .iter()
        .find(|e| e.0 == engine)
        .copied()
        .ok_or_else(|| format!("{engine} 还不支持多账号。"))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountBook {
    /// 引擎 → 当前账号 id（没记就是默认）。
    pub current: BTreeMap<String, String>,
    /// 引擎 → 新增过的账号 id。
    pub added: BTreeMap<String, Vec<String>>,
}

impl AccountBook {
    pub fn load(store: &Store) -> Self {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, store: &Store) -> Result<(), String> {
        let raw = serde_json::to_string(self).map_err(|_| "存不了账号表。")?;
        store.set_setting(KEY, &raw).map_err(|_| "存不了账号表。".into())
    }

    pub fn current(&self, engine: &str) -> String {
        self.current
            .get(engine)
            .filter(|id| *id == DEFAULT_ID || self.has(engine, id))
            .cloned()
            .unwrap_or_else(|| DEFAULT_ID.into())
    }

    fn has(&self, engine: &str, id: &str) -> bool {
        self.added.get(engine).is_some_and(|v| v.iter().any(|a| a == id))
    }
}

/// 账号名：1–32 个字，不能有 `/`、`\`、不能以 `.` 开头，不能叫 `default`。
pub fn valid_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=32).contains(&n)
        && !name.starts_with('.')
        && name != DEFAULT_ID
        && name != "默认"
        && name.chars().all(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':'))
        && name.trim() == name
}

/// 账号目录的根：「CLI 目录」旁边的 `accounts/`，没设就放 Peck 数据目录。
pub fn root(config: &GatewayConfig) -> PathBuf {
    match config.cli_dir_path() {
        Some(dir) => dir.parent().map(Path::to_path_buf).unwrap_or(dir).join("accounts"),
        None => crate::mode::app_data_dir().join("accounts"),
    }
}

pub fn dir(config: &GatewayConfig, engine: &str, id: &str) -> Option<PathBuf> {
    (id != DEFAULT_ID).then(|| root(config).join(engine).join(id))
}

/// 驱动这个引擎时要加的环境变量（当前账号的配置目录）。默认账号不加。
pub fn env_for(book: &AccountBook, config: &GatewayConfig, engine: &str) -> Vec<(String, String)> {
    let Ok((_, _, _, var)) = spec(engine) else {
        return vec![];
    };
    match dir(config, engine, &book.current(engine)) {
        Some(d) => vec![(var.to_string(), d.to_string_lossy().into_owned())],
        None => vec![],
    }
}

/// 当前账号的显示名（模型列表用）；默认账号为空。
pub fn current_label(book: &AccountBook, engine: &str) -> Option<String> {
    let id = book.current(engine);
    (id != DEFAULT_ID).then_some(id)
}

// ───────────────────────── 给设置页的 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub logged_in: bool,
    /// 订阅类型（Claude Code 报的 `max`、`pro`；Codex 是「ChatGPT」）。
    #[serde(default)]
    pub plan: Option<String>,
    /// 登录的邮箱（Claude Code 报的；只给站主自己看）。
    #[serde(default)]
    pub email: Option<String>,
    /// 一句话：状态命令的结论或出错原因。
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAccount {
    pub id: String,
    /// 显示名（默认账号是「默认」）。
    pub label: String,
    pub is_default: bool,
    pub current: bool,
    #[serde(default)]
    pub status: Option<AccountStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineAccounts {
    pub engine: String,
    pub label: String,
    /// 默认账号和谁共用登录（界面写明）。
    pub default_note: String,
    pub accounts: Vec<CliAccount>,
}

fn default_note(engine: &str) -> String {
    match engine {
        "codex" => "默认账号用 ~/.codex，和 ChatGPT 客户端共用登录".into(),
        _ => "默认账号用 ~/.claude，和 Claude 客户端的 Code 页共用设置".into(),
    }
}

pub fn list(book: &AccountBook) -> Vec<EngineAccounts> {
    ENGINES
        .iter()
        .map(|(engine, label, _, _)| {
            let current = book.current(engine);
            let mut accounts = vec![CliAccount {
                id: DEFAULT_ID.into(),
                label: "默认".into(),
                is_default: true,
                current: current == DEFAULT_ID,
                status: None,
            }];
            for id in book.added.get(*engine).into_iter().flatten() {
                accounts.push(CliAccount {
                    id: id.clone(),
                    label: id.clone(),
                    is_default: false,
                    current: *id == current,
                    status: None,
                });
            }
            EngineAccounts {
                engine: engine.to_string(),
                label: label.to_string(),
                default_note: default_note(engine),
                accounts,
            }
        })
        .collect()
}

pub fn add(book: &mut AccountBook, config: &GatewayConfig, engine: &str, name: &str) -> Result<(), String> {
    spec(engine)?;
    let name = name.trim();
    if !valid_name(name) {
        return Err("账号名 1–32 个字，不能带 / \\ :，不能叫「默认」。".into());
    }
    if book.has(engine, name) {
        return Err(format!("已经有叫「{name}」的账号了。"));
    }
    let d = dir(config, engine, name).ok_or("账号名不对。")?;
    std::fs::create_dir_all(&d).map_err(|_| "建不了账号目录：看看 CLI 目录能不能写。")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    book.added.entry(engine.into()).or_default().push(name.into());
    Ok(())
}

pub fn use_account(book: &mut AccountBook, engine: &str, id: &str) -> Result<(), String> {
    spec(engine)?;
    if id != DEFAULT_ID && !book.has(engine, id) {
        return Err("没有这个账号。".into());
    }
    book.current.insert(engine.into(), id.into());
    Ok(())
}

/// 删掉一个新增的账号和它的目录（里面的登录一起没了）。默认账号不能删。
pub fn remove(book: &mut AccountBook, config: &GatewayConfig, engine: &str, id: &str) -> Result<(), String> {
    spec(engine)?;
    if id == DEFAULT_ID {
        return Err("默认账号不能删。".into());
    }
    if !book.has(engine, id) || !valid_name(id) {
        return Err("没有这个账号。".into());
    }
    if let Some(d) = dir(config, engine, id) {
        if d.exists() {
            std::fs::remove_dir_all(&d).map_err(|_| "删不掉账号目录。")?;
        }
    }
    if let Some(v) = book.added.get_mut(engine) {
        v.retain(|a| a != id);
    }
    if book.current.get(engine).is_some_and(|c| c == id) {
        book.current.remove(engine);
    }
    Ok(())
}

/// 重新验权：只跑状态命令，不发模型请求。
pub async fn check(config: &GatewayConfig, engine: &str, id: &str) -> AccountStatus {
    let Ok((_, label, command, var)) = spec(engine) else {
        return AccountStatus { logged_in: false, plan: None, email: None, detail: "不支持".into() };
    };
    let Some(setup) = super::engines::locate(command, config).await else {
        return AccountStatus { logged_in: false, plan: None, email: None, detail: format!("没找到 {label}") };
    };
    let mut cmd = tokio::process::Command::new(&setup.bin);
    match engine {
        "claude-code" => cmd.args(["auth", "status", "--json"]),
        _ => cmd.args(["login", "status"]),
    };
    cmd.env("PATH", &setup.path)
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(d) = dir(config, engine, id) {
        let _ = std::fs::create_dir_all(&d);
        cmd.env(var, d);
    }
    let output = match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(_)) => return AccountStatus { logged_in: false, plan: None, email: None, detail: format!("{label} 起不来") },
        Err(_) => return AccountStatus { logged_in: false, plan: None, email: None, detail: "状态命令超时了".into() },
    };
    parse_status(engine, output.status.success(), &String::from_utf8_lossy(&output.stdout), &String::from_utf8_lossy(&output.stderr))
}

pub fn parse_status(engine: &str, ok: bool, stdout: &str, stderr: &str) -> AccountStatus {
    if engine == "claude-code" {
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
        let logged_in = v["loggedIn"].as_bool().unwrap_or(false);
        return AccountStatus {
            logged_in,
            plan: v["subscriptionType"].as_str().map(str::to_string),
            email: v["email"].as_str().map(str::to_string),
            detail: if logged_in {
                match v["authMethod"].as_str() {
                    Some("claude.ai") => "已登录（Claude 订阅）".into(),
                    Some(other) => format!("已登录（{other}）"),
                    None => "已登录".into(),
                }
            } else {
                "没登录".into()
            },
        };
    }
    let text = format!("{stdout}\n{stderr}");
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("Logged in") || l.starts_with("Not logged in"))
        .unwrap_or("");
    let logged_in = ok && line.starts_with("Logged in");
    AccountStatus {
        logged_in,
        plan: (logged_in && line.contains("ChatGPT")).then(|| "ChatGPT".into()),
        email: None,
        detail: if logged_in {
            if line.contains("API key") { "已登录（API Key）".into() } else { "已登录（ChatGPT）".into() }
        } else {
            "没登录".into()
        },
    }
}

// ───────────────────────── 登录 ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    pub running: bool,
    /// 登录链接（站主复制到自己挑的浏览器里打开）。
    #[serde(default)]
    pub url: Option<String>,
    /// 登录命令的输出（最后一段，去掉了终端控制符）。Codex 的一次性代码在这里。
    pub output: String,
    /// 结束了：成功为真。
    #[serde(default)]
    pub ok: Option<bool>,
}

struct LoginJob {
    state: Arc<Mutex<LoginState>>,
    url_file: PathBuf,
    killer: Box<dyn ChildKiller + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

fn jobs() -> &'static Mutex<HashMap<(String, String), LoginJob>> {
    static JOBS: std::sync::OnceLock<Mutex<HashMap<(String, String), LoginJob>>> = std::sync::OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 只记下链接、不开浏览器的小脚本（`open` 和 `BROWSER` 都指向它）。
fn shim_dir() -> Result<PathBuf, String> {
    let d = crate::mode::app_data_dir().join("login-shim");
    std::fs::create_dir_all(&d).map_err(|_| "建不了登录用的临时目录。")?;
    let script = "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$PECK_LOGIN_URL_FILE\"; done\n";
    for name in ["open", "xdg-open", "peck-browser"] {
        let p = d.join(name);
        std::fs::write(&p, script).map_err(|_| "写不了登录用的小脚本。")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    Ok(d)
}

/// 开始登录（切换登录）：在伪终端里跑 CLI 的登录命令。同一个账号再点一次先停掉上一次。
pub async fn login_start(config: &GatewayConfig, engine: &str, id: &str) -> Result<LoginState, String> {
    let cli_dir = config.cli_dir.clone();
    login_start_with(config, &cli_dir, engine, id).await
}

/// `cli_dir` 用来找命令，`config` 用来算账号目录（测试里两者分开）。
async fn login_start_with(config: &GatewayConfig, cli_dir: &str, engine: &str, id: &str) -> Result<LoginState, String> {
    let (_, label, command, var) = spec(engine)?;
    let finder = GatewayConfig {
        cli_dir: cli_dir.into(),
        ..config.clone()
    };
    let setup = super::engines::locate(command, &finder)
        .await
        .ok_or_else(|| format!("没找到 {label}：先在上面填 CLI 目录。"))?;
    login_cancel(engine, id);
    let shims = shim_dir()?;
    let url_file = std::env::temp_dir().join(format!("peck-login-{}.txt", uuid::Uuid::new_v4()));
    let pair = native_pty_system()
        .openpty(PtySize { rows: 30, cols: 200, pixel_width: 0, pixel_height: 0 })
        .map_err(|_| "登录窗口起不来。")?;
    let mut builder = CommandBuilder::new(&setup.bin);
    match engine {
        "claude-code" => builder.args(["auth", "login", "--claudeai"]),
        _ => builder.args(["login", "--device-auth"]),
    }
    let path = format!("{}:{}", shims.to_string_lossy(), setup.path);
    builder.env("PATH", path);
    builder.env("BROWSER", shims.join("peck-browser"));
    builder.env("PECK_LOGIN_URL_FILE", &url_file);
    builder.env("DISABLE_AUTOUPDATER", "1");
    builder.env("TERM", "dumb");
    builder.env("NO_COLOR", "1");
    if let Ok(home) = std::env::var("HOME") {
        builder.env("HOME", home);
        builder.cwd(std::env::var("HOME").unwrap_or_default());
    }
    if let Some(d) = dir(config, engine, id) {
        std::fs::create_dir_all(&d).map_err(|_| "建不了账号目录。")?;
        builder.env(var, d);
    }
    let mut child = pair.slave.spawn_command(builder).map_err(|_| format!("{label} 的登录命令起不来。"))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|_| "读不了登录窗口。")?;
    let writer = pair.master.take_writer().map_err(|_| "写不了登录窗口。")?;
    let killer = child.clone_killer();
    let state = Arc::new(Mutex::new(LoginState { running: true, ..Default::default() }));

    let out = state.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut pending = Vec::new();
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&pending).into_owned();
            pending.clear();
            if let Ok(mut s) = out.lock() {
                s.output.push_str(&strip_ansi(&text));
                let len = s.output.chars().count();
                if len > 4000 {
                    s.output = s.output.chars().skip(len - 4000).collect();
                }
                if s.url.is_none() {
                    s.url = find_url(&s.output);
                }
            }
        }
    });
    let done = state.clone();
    std::thread::spawn(move || {
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        if let Ok(mut s) = done.lock() {
            s.running = false;
            s.ok = Some(ok);
        }
        drop(pair.master);
    });
    jobs()
        .lock()
        .map_err(|_| "登录状态乱了。")?
        .insert((engine.into(), id.into()), LoginJob { state, url_file, killer, writer });
    // 给它一点时间吐出链接。
    for _ in 0..30 {
        let s = login_state(engine, id);
        if s.url.is_some() || !s.running {
            return Ok(s);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(login_state(engine, id))
}

pub fn login_state(engine: &str, id: &str) -> LoginState {
    let Ok(jobs) = jobs().lock() else {
        return LoginState::default();
    };
    let Some(job) = jobs.get(&(engine.to_string(), id.to_string())) else {
        return LoginState::default();
    };
    let mut s = job.state.lock().map(|s| s.clone()).unwrap_or_default();
    if let Ok(text) = std::fs::read_to_string(&job.url_file) {
        if let Some(url) = text.lines().find(|l| l.starts_with("https://")) {
            s.url = Some(url.trim().to_string());
        }
    }
    s
}

/// 往登录窗口里输一行（Claude Code 要粘 code 的时候）。
pub fn login_input(engine: &str, id: &str, text: &str) -> Result<(), String> {
    let mut jobs = jobs().lock().map_err(|_| "登录状态乱了。")?;
    let job = jobs.get_mut(&(engine.to_string(), id.to_string())).ok_or("这个账号没在登录。")?;
    job.writer
        .write_all(format!("{}\r", text.trim()).as_bytes())
        .and_then(|_| job.writer.flush())
        .map_err(|_| "输不进去：登录可能已经结束了。".into())
}

pub fn login_cancel(engine: &str, id: &str) {
    if let Ok(mut jobs) = jobs().lock() {
        if let Some(mut job) = jobs.remove(&(engine.to_string(), id.to_string())) {
            let _ = job.killer.kill();
            let _ = std::fs::remove_file(&job.url_file);
        }
    }
}

fn find_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',', ')', '"', '\'']).to_string())
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() || d == '~' {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                for d in chars.by_ref() {
                    if d == '\u{7}' {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

// ───────────────────────── IPC ─────────────────────────

fn load(state: &crate::state::AppState) -> Result<(AccountBook, GatewayConfig), String> {
    let store = state.store.lock().map_err(|_| "读不到设置。")?;
    let config = GatewayConfig::load(&store).map_err(|_| "读不到设置。")?;
    Ok((AccountBook::load(&store), config))
}

fn save(state: &crate::state::AppState, book: &AccountBook) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "存不了设置。")?;
    book.save(&store)
}

/// 账号列表；`check` 为真就顺手给每个账号跑一次状态命令。
#[tauri::command]
pub async fn cli_accounts_list(check_all: bool, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<EngineAccounts>, String> {
    let (book, config) = load(&state)?;
    let mut out = list(&book);
    if check_all {
        for group in &mut out {
            for account in &mut group.accounts {
                account.status = Some(check(&config, &group.engine, &account.id).await);
            }
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn cli_account_add(engine: String, name: String, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<EngineAccounts>, String> {
    let (mut book, config) = load(&state)?;
    add(&mut book, &config, &engine, &name)?;
    save(&state, &book)?;
    Ok(list(&book))
}

#[tauri::command]
pub async fn cli_account_use(engine: String, id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<EngineAccounts>, String> {
    let (mut book, _) = load(&state)?;
    use_account(&mut book, &engine, &id)?;
    save(&state, &book)?;
    Ok(list(&book))
}

#[tauri::command]
pub async fn cli_account_remove(engine: String, id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<EngineAccounts>, String> {
    let (mut book, config) = load(&state)?;
    login_cancel(&engine, &id);
    remove(&mut book, &config, &engine, &id)?;
    save(&state, &book)?;
    Ok(list(&book))
}

#[tauri::command]
pub async fn cli_account_check(engine: String, id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<AccountStatus, String> {
    let (_, config) = load(&state)?;
    Ok(check(&config, &engine, &id).await)
}

#[tauri::command]
pub async fn cli_account_login(engine: String, id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<LoginState, String> {
    let (book, config) = load(&state)?;
    if id != DEFAULT_ID && !book.has(&engine, &id) {
        return Err("没有这个账号。".into());
    }
    login_start(&config, &engine, &id).await
}

#[tauri::command]
pub async fn cli_account_login_state(engine: String, id: String) -> Result<LoginState, String> {
    Ok(login_state(&engine, &id))
}

#[tauri::command]
pub async fn cli_account_login_input(engine: String, id: String, text: String) -> Result<(), String> {
    login_input(&engine, &id, &text)
}

#[tauri::command]
pub async fn cli_account_login_cancel(engine: String, id: String) -> Result<(), String> {
    login_cancel(&engine, &id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(cli_dir: &str) -> GatewayConfig {
        GatewayConfig {
            cli_dir: cli_dir.into(),
            ..Default::default()
        }
    }

    #[test]
    fn accounts_live_next_to_the_cli_dir_and_set_the_right_variable() {
        let tmp = std::env::temp_dir().join(format!("peck-acct-{}", uuid::Uuid::new_v4()));
        let bin = tmp.join("cli/bin");
        let cfg = config(&bin.to_string_lossy());
        assert_eq!(root(&cfg), tmp.join("cli/accounts"));
        let mut book = AccountBook::default();
        assert!(env_for(&book, &cfg, "claude-code").is_empty(), "默认账号不加环境变量");
        add(&mut book, &cfg, "claude-code", "工作").unwrap();
        assert!(tmp.join("cli/accounts/claude-code/工作").is_dir());
        assert!(add(&mut book, &cfg, "claude-code", "工作").is_err());
        assert!(add(&mut book, &cfg, "claude-code", "../x").is_err());
        assert!(add(&mut book, &cfg, "claude-code", "默认").is_err());
        assert!(add(&mut book, &cfg, "muse", "a").is_err());
        use_account(&mut book, "claude-code", "工作").unwrap();
        let env = env_for(&book, &cfg, "claude-code");
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        assert!(env[0].1.ends_with("cli/accounts/claude-code/工作"));
        assert!(env_for(&book, &cfg, "codex").is_empty(), "各引擎分开");
        assert_eq!(current_label(&book, "claude-code").as_deref(), Some("工作"));

        let groups = list(&book);
        let claude = groups.iter().find(|g| g.engine == "claude-code").unwrap();
        assert_eq!(claude.accounts.len(), 2);
        assert!(claude.accounts[1].current && !claude.accounts[0].current);

        assert!(remove(&mut book, &cfg, "claude-code", DEFAULT_ID).is_err());
        remove(&mut book, &cfg, "claude-code", "工作").unwrap();
        assert!(!tmp.join("cli/accounts/claude-code/工作").exists());
        assert_eq!(book.current("claude-code"), DEFAULT_ID, "删了当前账号就回到默认");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn status_output_is_read_without_touching_credentials() {
        let s = parse_status("claude-code", true, r#"{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max", "email": "a@b.c"}"#, "");
        assert!(s.logged_in);
        assert_eq!(s.plan.as_deref(), Some("max"));
        let s = parse_status("claude-code", true, r#"{"loggedIn": false, "authMethod": "none"}"#, "");
        assert!(!s.logged_in && s.detail == "没登录");
        let s = parse_status("codex", true, "Logged in using ChatGPT\n", "");
        assert!(s.logged_in && s.plan.as_deref() == Some("ChatGPT"));
        let s = parse_status("codex", false, "Not logged in\n", "");
        assert!(!s.logged_in);
    }

    /// 本地试登录流程（不真登录，平时不跑）：`PECK_LIVE_CLI_DIR=<cli/bin>`。临时账号目录、拿到链接就取消。
    #[tokio::test]
    #[ignore]
    async fn live_login_shows_a_link_without_opening_a_browser() {
        let tmp = std::env::temp_dir().join(format!("peck-login-{}", uuid::Uuid::new_v4()));
        let cli_dir = std::env::var("PECK_LIVE_CLI_DIR").expect("PECK_LIVE_CLI_DIR");
        // 账号根放临时目录（借一个假的 CLI 目录算根），命令照真 CLI 目录找。
        let cfg = config(&tmp.join("cli/bin").to_string_lossy());
        let mut book = AccountBook::default();
        add(&mut book, &cfg, "claude-code", "试登录").unwrap();
        let state = login_start_with(&cfg, &cli_dir, "claude-code", "试登录").await.unwrap();
        println!("running={} url={:?}", state.running, state.url.as_deref().map(|u| u.split('?').next().unwrap_or("")));
        println!("output: {}", state.output.chars().take(300).collect::<String>());
        login_cancel("claude-code", "试登录");
        assert!(state.url.is_some_and(|u| u.starts_with("https://")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn login_output_is_cleaned_and_links_are_found() {
        let raw = "\u{1b}[2J\u{1b}[1mOpen this link:\u{1b}[0m\r\nhttps://auth.example.com/device?x=1.\r\nCode: ABCD-1234\r\n";
        let clean = strip_ansi(raw);
        assert!(clean.contains("Code: ABCD-1234") && !clean.contains('\u{1b}'));
        assert_eq!(find_url(&clean).as_deref(), Some("https://auth.example.com/device?x=1"));
    }
}
