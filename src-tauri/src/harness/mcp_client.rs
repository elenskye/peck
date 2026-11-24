//! 外部 MCP 服务器（v0.8 Phase 11）：站主在设置里配的 stdio MCP 服务器（命令 + 参数 + 环境变量）。
//! Peck Loop 发任务时起（ONE SHOT 不起），工具以 `mcp__<服务器>__<工具>` 给模型，任务结束一起停。
//!
//! - 协议：一行一条 JSON-RPC（`initialize` → `notifications/initialized` → `tools/list` / `tools/call`）。
//!   服务器发来的请求（`ping` 回空、别的回「没有这个方法」），通知不理。
//! - 权限：PLAN 只给标了只读（`readOnlyHint`）的工具；EDIT 用没标只读的工具前问一次（能「这个项目总是允许」
//!   这台服务器，规则 `mcp:<服务器>`）；AUTO 不问；会删改的（标了 `destructiveHint`，或者只读、删改都没标——照 MCP 规范的默认）
//!   哪一档都当高危问。
//! - 服务器是站主自己配的程序，在 Peck 的沙箱外跑（和 CLI 引擎一样），环境是站主 shell 的那份加上配置里写的。
//! - 配置存在 `app_settings` 的 `mcp_servers`；要保密的环境变量（Key 这类）只记名字，值放钥匙串（v0.10，`secretEnv`）。
//! - **项目里的 MCP**（v0.10，roadmap A2）：项目根的 `.mcp.json`（Claude Code 的格式，`mcpServers.<名字>.command / args / env`，
//!   `${VAR}`、`${VAR:-默认}` 照它的规矩展开）也读，一份配置四家都用；同名的以项目的为准，项目设置里能一个个关掉
//!   （`ProjectSettings.mcp_off`，设置里的也能对这个项目关）。项目里的是项目里的程序、在沙箱外跑：第一次起之前问一次站主，
//!   「总是允许」按项目 + 名字 + 配置的指纹记（`mcp-project:<名字>:<指纹>`），配置改了再问。只接 stdio 的；`url` 那种跳过、写一句。
//! - **工具标注记下来**（`mcp_hints`）：Peck 起过、TEST 过的服务器，每个工具是只读、会改、会删改，按项目记。
//!   Grok 经 ACP 问要不要调 MCP 工具时不带标注，Peck 照这份替它们批（`gateway::runner`）。

use crate::store::{Store, StoreResult};
use crate::workspace::EnvVar;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::oneshot;

pub const KEY: &str = "mcp_servers";
pub const MAX_SERVERS: usize = 8;
const START_TIMEOUT: Duration = Duration::from_secs(20);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// 工具结果最多给模型多少字节。
const MAX_OUTPUT: usize = 40_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// 名字（工具名前缀，`[a-z0-9_-]`，最长 24）。
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// 值放在钥匙串里的环境变量（v0.10）：只记名字，起服务器时从钥匙串取（`mcp:<服务器>:<名字>`）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_env: Vec<String>,
    /// 从项目的 `.mcp.json` 读来的（v0.10 A2）：那一项配置的指纹。设置里存的没有这个。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 项目 `.mcp.json` 里写的原文（命令、参数，`${VAR}` 没展开）：起之前问站主的卡片给它看，
    /// 展开后的值可能是 Token，不进事件日志（v0.10 收尾审查）。不存。
    #[serde(skip)]
    pub raw: Option<(String, Vec<String>)>,
}

fn yes() -> bool {
    true
}

impl McpServerConfig {
    pub fn new(name: &str, command: &str, args: Vec<String>, env: Vec<EnvVar>) -> Self {
        Self { name: name.into(), command: command.into(), args, env, enabled: true, secret_env: vec![], project: None, raw: None }
    }

    /// 项目里的服务器第一次起之前要问；「总是允许」记的规则（配置改了指纹就变，再问一次）。
    pub fn project_rule(&self) -> Option<String> {
        self.project.as_ref().map(|fp| format!("{PROJECT_RULE}{}:{fp}", self.name))
    }
}

/// 项目里的 MCP 服务器「总是允许」规则的前缀。
pub const PROJECT_RULE: &str = "mcp-project:";
/// 钥匙串里放 MCP 环境变量的 id：`mcp:<服务器>:<名字>`。
pub fn secret_id(server: &str, name: &str) -> String {
    format!("mcp:{server}:{name}")
}

pub fn load(store: &Store) -> StoreResult<Vec<McpServerConfig>> {
    Ok(store.setting(KEY)?.and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default())
}

/// 规整、校验后存。
pub fn save(store: &Store, list: Vec<McpServerConfig>) -> Result<Vec<McpServerConfig>, String> {
    let list = normalized(list)?;
    store.set_setting(KEY, &serde_json::to_string(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(list)
}

pub fn normalized(list: Vec<McpServerConfig>) -> Result<Vec<McpServerConfig>, String> {
    if list.len() > MAX_SERVERS {
        return Err(format!("最多 {MAX_SERVERS} 个 MCP 服务器。"));
    }
    let mut out: Vec<McpServerConfig> = Vec::new();
    for mut s in list {
        s.name = s.name.trim().to_lowercase();
        s.command = s.command.trim().to_string();
        s.args = s.args.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
        if s.name.is_empty() || s.name.len() > 24 || !s.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("MCP 服务器的名字只能用小写字母、数字、- 和 _，最长 24：{}", s.name));
        }
        if out.iter().any(|o| o.name == s.name) {
            return Err(format!("有两个叫 {} 的 MCP 服务器。", s.name));
        }
        if s.command.is_empty() {
            return Err(format!("{} 还没写命令。", s.name));
        }
        for v in &s.env {
            if v.name.trim().is_empty() || !v.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(format!("{} 的环境变量名不对：{}", s.name, v.name));
            }
        }
        s.secret_env = s.secret_env.into_iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
        s.secret_env.sort();
        s.secret_env.dedup();
        for n in &s.secret_env {
            if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(format!("{} 的环境变量名不对：{n}", s.name));
            }
        }
        // 钥匙串里有的，明文那份不留。
        s.env.retain(|v| !s.secret_env.contains(&v.name));
        // 项目里读来的不存进设置。
        s.project = None;
        out.push(s);
    }
    Ok(out)
}

/// 项目里最多读几个服务器。
pub const MAX_PROJECT_SERVERS: usize = 8;

/// 项目根 `.mcp.json` 里读到的（v0.10 A2）。
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcp {
    pub servers: Vec<McpServerConfig>,
    /// 读了但用不了的：（名字，为什么）。
    pub skipped: Vec<(String, String)>,
    /// `.mcp.json` 本身读不懂（写错了）：为什么。
    pub error: Option<String>,
}

/// 名字规整成工具名能用的（小写字母、数字、- 和 _，最长 24）。
fn clean_name(raw: &str) -> String {
    let mut n: String = raw.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    n.truncate(24);
    n
}

/// 照 Claude Code 的规矩展开 `${VAR}`、`${VAR:-默认}`（没有这个变量、也没写默认的，换成空）。
pub fn expand_vars(text: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if valid {
            out.push_str(&env(name).filter(|v| !v.is_empty()).or_else(|| default.map(str::to_string)).unwrap_or_default());
        } else {
            out.push_str(&rest[at..at + 2 + end + 1]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// 读项目根的 `.mcp.json`：stdio 的（有 `command`）收下，`url` 那种（http、sse）跳过。`env` 用来展开 `${VAR}`
/// （站主 shell 的那份 + 项目的环境变量）。指到项目外的符号链接不认。
pub fn read_project(root: &Path, env: &[(String, String)]) -> ProjectMcp {
    let mut out = ProjectMcp::default();
    let path = root.join(".mcp.json");
    let inside = path.canonicalize().ok().zip(root.canonicalize().ok()).is_some_and(|(p, r)| p.starts_with(r));
    if !inside {
        return out;
    }
    let Ok(text) = std::fs::read_to_string(&path) else { return out };
    if text.len() > 256 * 1024 {
        out.error = Some(".mcp.json 太大了（超过 256KB），没读。".into());
        return out;
    }
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            out.error = Some(format!(".mcp.json 读不懂：{e}"));
            return out;
        }
    };
    let Some(map) = parsed.get("mcpServers").and_then(Value::as_object) else { return out };
    let lookup = |k: &str| env.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    for (raw_name, entry) in map {
        let name = clean_name(raw_name);
        if name.is_empty() {
            out.skipped.push((raw_name.clone(), "名字用不了".into()));
            continue;
        }
        if out.servers.iter().any(|s| s.name == name) {
            out.skipped.push((raw_name.clone(), format!("和另一个服务器都叫 {name}")));
            continue;
        }
        let kind = entry.get("type").and_then(Value::as_str).unwrap_or("stdio");
        let Some(command) = entry.get("command").and_then(Value::as_str).filter(|c| !c.trim().is_empty() && kind == "stdio") else {
            out.skipped.push((raw_name.clone(), "Peck Loop 只接 stdio 的 MCP 服务器（这个是网址那种，CLI 自己读）".into()));
            continue;
        };
        if out.servers.len() >= MAX_PROJECT_SERVERS {
            out.skipped.push((raw_name.clone(), format!("一个项目最多读 {MAX_PROJECT_SERVERS} 个")));
            continue;
        }
        let args: Vec<String> = entry
            .get("args")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(|a| expand_vars(a, &lookup)).collect())
            .unwrap_or_default();
        let vars: Vec<EnvVar> = entry
            .get("env")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                    .filter_map(|(k, v)| v.as_str().map(|v| EnvVar { name: k.clone(), value: expand_vars(v, &lookup) }))
                    .collect()
            })
            .unwrap_or_default();
        // 指纹按原文算（展开以前）：命令、参数、环境变量的写法改了就再问一次。
        // 指纹用完整的 md5（以前只取前 8 位，凑得出同指纹的改过的配置；v0.10 收尾审查）。
        let fingerprint = crate::workspace::md5_hex(entry.to_string().as_bytes());
        let raw_args: Vec<String> = entry.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let mut server = McpServerConfig::new(&name, &expand_vars(command, &lookup), args, vars);
        server.project = Some(fingerprint);
        server.raw = Some((command.to_string(), raw_args));
        out.servers.push(server);
    }
    out
}

/// 这个项目的 Peck Loop 要起的：设置里开着的 + 项目里的（同名的以项目的为准），再去掉项目设置里关掉的。
pub fn merged(global: &[McpServerConfig], project: &[McpServerConfig], off: &[String]) -> Vec<McpServerConfig> {
    let mut out: Vec<McpServerConfig> = global.iter().filter(|g| g.enabled && !project.iter().any(|p| p.name == g.name)).cloned().collect();
    out.extend(project.iter().cloned());
    out.retain(|s| !off.contains(&s.name));
    out
}

/// 钥匙串里的环境变量填进去（取不到的写进第二个返回值：哪台、哪个变量）。
pub fn with_secrets(mut list: Vec<McpServerConfig>, keys: &std::sync::Mutex<crate::api::keys::Keys>) -> (Vec<McpServerConfig>, Vec<String>) {
    let mut missing = Vec::new();
    for s in &mut list {
        for n in s.secret_env.clone() {
            let value = keys.lock().ok().and_then(|mut k| k.get(&secret_id(&s.name, &n)).ok());
            match value {
                Some(v) => s.env.push(EnvVar { name: n, value: v.to_string() }),
                None => missing.push(format!("{} 的 {n}", s.name)),
            }
        }
    }
    (list, missing)
}

/// 钥匙串（App 起来时交过来；测试里没有，钥匙串里的变量就当取不到）。
static API: std::sync::OnceLock<Arc<crate::api::ApiState>> = std::sync::OnceLock::new();

pub fn set_keys(api: Arc<crate::api::ApiState>) {
    let _ = API.set(api);
}

/// 一个项目的 Peck Loop 要起的 MCP：设置里开着的 + 项目 `.mcp.json` 里的（同名的以项目的为准），去掉项目设置里关掉的。
/// 第二个是任务开头要写的提示（`.mcp.json` 写错了）。钥匙串里的变量另用 [`fill_secrets`] 填（别拿着数据库的锁读钥匙串）。
pub fn for_project(store: &Store, ws: &crate::workspace::Workspace) -> (Vec<McpServerConfig>, Vec<String>) {
    let global = load(store).unwrap_or_default();
    let project = read_project(Path::new(&ws.path), &project_env(ws));
    let mut notes = Vec::new();
    if let Some(e) = &project.error {
        notes.push(format!("{e}项目里的 MCP 这次没起。"));
    }
    (merged(&global, &project.servers, &ws.settings.mcp_off), notes)
}

/// 钥匙串里的变量填进去；取不到的在 `notes` 里写一行。
pub fn fill_secrets(list: Vec<McpServerConfig>, notes: &mut Vec<String>) -> Vec<McpServerConfig> {
    if list.iter().all(|s| s.secret_env.is_empty()) {
        return list;
    }
    let (list, missing) = match API.get() {
        Some(api) => with_secrets(list, &api.keys),
        None => {
            let missing = list.iter().flat_map(|s| s.secret_env.iter().map(move |n| format!("{} 的 {n}", s.name))).collect();
            (list, missing)
        }
    };
    if !missing.is_empty() {
        notes.push(format!("钥匙串里取不到 {}，这几个变量没带上。", missing.join("、")));
    }
    list
}

/// 服务器起来以后记下工具标注（项目里的按项目记，设置里的记在 `*`）。
pub fn seen_hook(store: Arc<Mutex<Store>>, workspace_id: String) -> McpSeen {
    Arc::new(move |hub: &McpHub, configs: &[McpServerConfig]| {
        let Ok(db) = store.lock() else { return };
        for c in configs {
            if hub.client(&c.name).is_none() {
                continue;
            }
            let scope = if c.project.is_some() { workspace_id.as_str() } else { "*" };
            hints_record(&db, scope, &c.name, &hub.tools);
        }
    })
}

/// 一个工具是只读、会改、还是会删改（照服务器的标注；没标的照 MCP 规范的默认算会删改）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolHint {
    Read,
    Write,
    Destructive,
}

impl ToolHint {
    pub fn of(tool: &McpTool) -> Self {
        if tool.read_only {
            Self::Read
        } else if tool.destructive {
            Self::Destructive
        } else {
            Self::Write
        }
    }
}

/// 存着的标注：范围（`*` 是设置里的，或者项目 id）→ 服务器 → 工具 → 标注。
pub const HINTS_KEY: &str = "mcp_hints";
pub type Hints = std::collections::BTreeMap<String, std::collections::BTreeMap<String, std::collections::BTreeMap<String, ToolHint>>>;

fn hints_load(store: &Store) -> Hints {
    store.setting(HINTS_KEY).ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

/// 记下一台服务器的工具标注（整台换掉，旧的工具不留）。`scope`：设置里的是 `*`，项目里的是项目 id。
pub fn hints_record(store: &Store, scope: &str, server: &str, tools: &[McpTool]) {
    let mut all = hints_load(store);
    let entry: std::collections::BTreeMap<String, ToolHint> = tools.iter().filter(|t| t.server == server).map(|t| (t.name.clone(), ToolHint::of(t))).collect();
    all.entry(scope.to_string()).or_default().insert(server.to_string(), entry);
    if let Ok(raw) = serde_json::to_string(&all) {
        let _ = store.set_setting(HINTS_KEY, &raw);
    }
}

/// 一个项目看得到的标注：设置里的，被项目里同名的盖过。
pub fn hints_for(store: &Store, workspace_id: &str) -> std::collections::HashMap<String, std::collections::BTreeMap<String, ToolHint>> {
    let all = hints_load(store);
    let mut out: std::collections::HashMap<String, std::collections::BTreeMap<String, ToolHint>> = all.get("*").cloned().unwrap_or_default().into_iter().collect();
    if let Some(p) = all.get(workspace_id) {
        out.extend(p.clone());
    }
    out
}

/// 服务器给的一个工具。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub description: String,
    #[serde(skip)]
    pub input_schema: Value,
    pub read_only: bool,
    pub destructive: bool,
}

impl McpTool {
    /// 给模型的名字：`mcp__<服务器>__<工具>`（只留字母数字、- 和 _，最长 64）。
    pub fn full_name(&self) -> String {
        let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>();
        let mut n = format!("mcp__{}__{}", clean(&self.server), clean(&self.name));
        n.truncate(64);
        n
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// 一个起来了的服务器。
pub struct McpClient {
    pub name: String,
    child: Mutex<Option<Child>>,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    next: AtomicU64,
    reader: tokio::task::JoinHandle<()>,
}

impl McpClient {
    /// 起服务器、握手。`env` 是基础环境（站主 shell 的那份），配置里写的盖过它。
    pub async fn start(config: &McpServerConfig, env: &[(String, String)], cwd: &Path) -> Result<Self, String> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .current_dir(cwd)
            .env_clear()
            .envs(env.iter().filter(|(k, _)| !k.starts_with("PECK_")).map(|(k, v)| (k.clone(), v.clone())))
            .envs(config.env.iter().map(|v| (v.name.clone(), v.value.clone())))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|e| format!("MCP 服务器 {} 起不来：{e}", config.name))?;
        let stdin = child.stdin.take().ok_or("拿不到 MCP 服务器的输入。")?;
        let stdout = child.stdout.take().ok_or("拿不到 MCP 服务器的输出。")?;
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let p = pending.clone();
        let (back_tx, mut back_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { continue };
                let id = msg.get("id").and_then(Value::as_u64);
                match (id, msg.get("method")) {
                    // 服务器问我们：ping 回空，别的回没有这个方法。
                    (Some(_), Some(m)) => {
                        let reply = if m == "ping" {
                            json!({"jsonrpc": "2.0", "id": msg["id"], "result": {}})
                        } else {
                            json!({"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "Peck does not support this request."}})
                        };
                        let _ = back_tx.send(reply.to_string());
                    }
                    (Some(id), None) => {
                        let waiter = p.lock().ok().and_then(|mut w| w.remove(&id));
                        if let Some(w) = waiter {
                            let r = match (msg.get("result"), msg.get("error")) {
                                (Some(r), _) => Ok(r.clone()),
                                (None, Some(e)) => Err(e.get("message").and_then(Value::as_str).unwrap_or("MCP 服务器报错了。").to_string()),
                                _ => Err("MCP 服务器回的东西看不懂。".into()),
                            };
                            let _ = w.send(r);
                        }
                    }
                    _ => {}
                }
            }
            // 服务器退出了：还在等的都报错。
            if let Ok(mut w) = p.lock() {
                for (_, tx) in w.drain() {
                    let _ = tx.send(Err("MCP 服务器退出了。".into()));
                }
            }
        });
        // 服务器问我们的回答：和请求共用一个输入（弱引用：服务器停了这个后台任务跟着结束）。
        let writer = Arc::downgrade(&stdin);
        tokio::spawn(async move {
            while let Some(line) = back_rx.recv().await {
                let Some(w) = writer.upgrade() else { break };
                let mut w = w.lock().await;
                let _ = w.write_all(format!("{line}\n").as_bytes()).await;
                let _ = w.flush().await;
            }
        });
        let client = Self { name: config.name.clone(), child: Mutex::new(Some(child)), stdin, pending, next: AtomicU64::new(1), reader };
        let init = client
            .request(
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "peck", "version": env!("CARGO_PKG_VERSION")}}),
                START_TIMEOUT,
            )
            .await
            .map_err(|e| format!("MCP 服务器 {} 握手没成：{e}", config.name))?;
        let _ = init;
        client.notify("notifications/initialized", json!({})).await?;
        Ok(client)
    }

    async fn write(&self, msg: Value) -> Result<(), String> {
        let mut w = self.stdin.lock().await;
        w.write_all(format!("{msg}\n").as_bytes()).await.map_err(|e| format!("写不进 MCP 服务器 {}：{e}", self.name))?;
        w.flush().await.map_err(|e| e.to_string())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params})).await
    }

    pub async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id, tx);
        }
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})).await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(format!("MCP 服务器 {} 退出了。", self.name)),
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&id);
                }
                Err(format!("MCP 服务器 {} 过了 {} 秒没回。", self.name, timeout.as_secs()))
            }
        }
    }

    /// 列工具（翻页最多 10 页）。
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self.request("tools/list", params, START_TIMEOUT).await?;
            for t in r.get("tools").and_then(Value::as_array).cloned().unwrap_or_default() {
                let Some(name) = t.get("name").and_then(Value::as_str) else { continue };
                let ann = t.get("annotations");
                let hint = |k: &str| ann.and_then(|a| a.get(k)).and_then(Value::as_bool);
                // MCP 规范：没标 readOnlyHint 就不是只读；不是只读时 destructiveHint 没标算「会删改」。
                let read_only = hint("readOnlyHint").unwrap_or(false);
                out.push(McpTool {
                    server: self.name.clone(),
                    name: name.to_string(),
                    description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({"type": "object"})),
                    read_only,
                    destructive: !read_only && hint("destructiveHint").unwrap_or(true),
                });
            }
            cursor = r.get("nextCursor").and_then(Value::as_str).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }

    /// 调一个工具。
    pub async fn call(&self, tool: &str, args: Value) -> Result<McpResult, String> {
        let r = self.request("tools/call", json!({"name": tool, "arguments": args}), CALL_TIMEOUT).await?;
        Ok(McpResult::from_response(&r))
    }

    pub fn stop(&self) {
        self.reader.abort();
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    // 整个进程组一起停（npx 这类会再起子进程）。
                    unsafe {
                        libc_kill(-(pid as i32));
                    }
                }
                let _ = child.start_kill();
            }
        }
    }
}

/// 一次 MCP 工具调用的结果：给模型的文字、算不算出错、带回来的图（Peck Loop 交给能看图的模型）。
#[derive(Debug, Clone, Default)]
pub struct McpResult {
    pub text: String,
    pub is_error: bool,
    pub images: Vec<crate::gateway::ImageInput>,
}

impl McpResult {
    pub fn from_response(r: &Value) -> Self {
        let is_error = r.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let mut parts: Vec<String> = Vec::new();
        let mut images = Vec::new();
        for c in r.get("content").and_then(Value::as_array).cloned().unwrap_or_default() {
            match c.get("type").and_then(Value::as_str) {
                Some("text") => parts.push(c.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
                Some("image") => {
                    let media_type = c.get("mimeType").and_then(Value::as_str).unwrap_or("").to_string();
                    let data = c.get("data").and_then(Value::as_str).unwrap_or("").to_string();
                    // 最多带 4 张（和截图一样跟在这一轮的工具结果后面）；认不得的格式、太大的只留一句。
                    match (crate::gateway::ImageInput { media_type: media_type.clone(), data }).checked() {
                        Ok(img) if images.len() < 4 => {
                            images.push(img);
                            parts.push(format!("[图片 {media_type}，附在这一轮工具结果后面]"));
                        }
                        Ok(_) => parts.push(format!("[图片 {media_type}，一次最多带 4 张，这张没带]")),
                        Err(e) => parts.push(format!("[图片 {media_type}：{e}]")),
                    }
                }
                Some("resource") => {
                    let res = c.get("resource");
                    match res.and_then(|r| r.get("text")).and_then(Value::as_str) {
                        Some(t) => parts.push(t.to_string()),
                        None => parts.push(format!("[资源 {}]", res.and_then(|r| r.get("uri")).and_then(Value::as_str).unwrap_or(""))),
                    }
                }
                Some("resource_link") => parts.push(format!("[链接 {}]", c.get("uri").and_then(Value::as_str).unwrap_or(""))),
                _ => {}
            }
        }
        if parts.is_empty() {
            if let Some(s) = r.get("structuredContent") {
                parts.push(s.to_string());
            }
        }
        let text = parts.join("\n");
        let text = if text.len() > MAX_OUTPUT {
            format!("{}\n…（输出太长，后面 {} 字节没给）", super::tools::clip_bytes(&text, MAX_OUTPUT), text.len() - MAX_OUTPUT)
        } else {
            text
        };
        Self { text, is_error, images }
    }
}

#[cfg(unix)]
unsafe fn libc_kill(pgid: i32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pgid, 9);
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 服务器起来、列完工具以后（记标注用）：起来的这组和它们的配置。
pub type McpSeen = Arc<dyn Fn(&McpHub, &[McpServerConfig]) + Send + Sync>;

/// 一个任务起来的全部服务器和它们的工具。
#[derive(Default)]
pub struct McpHub {
    clients: Vec<Arc<McpClient>>,
    pub tools: Vec<McpTool>,
    /// 起不来的：（名字，原因）。
    pub failures: Vec<(String, String)>,
}

impl McpHub {
    /// 起所有开着的服务器（一起起，一个起不来不挡别的；v0.10 以前是一个一个起，几台加起来能等上一分钟）。
    pub async fn start(configs: &[McpServerConfig], env: &[(String, String)], cwd: &Path) -> Self {
        let mut hub = Self::default();
        let mut set = tokio::task::JoinSet::new();
        for (i, c) in configs.iter().filter(|c| c.enabled).cloned().enumerate() {
            let env = env.to_vec();
            let cwd = cwd.to_path_buf();
            set.spawn(async move {
                let r = match McpClient::start(&c, &env, &cwd).await {
                    Ok(client) => match client.list_tools().await {
                        Ok(tools) => Ok((client, tools)),
                        Err(e) => {
                            client.stop();
                            Err(e)
                        }
                    },
                    Err(e) => Err(e),
                };
                (i, c.name, r)
            });
        }
        let mut done = Vec::new();
        while let Some(r) = set.join_next().await {
            if let Ok(r) = r {
                done.push(r);
            }
        }
        // 按配置里的顺序（工具列表每次一样，缓存才命中）。
        done.sort_by_key(|(i, _, _)| *i);
        for (_, name, r) in done {
            match r {
                Ok((client, tools)) => {
                    hub.tools.extend(tools);
                    hub.clients.push(Arc::new(client));
                }
                Err(e) => hub.failures.push((name, e)),
            }
        }
        hub
    }

    pub fn find(&self, full_name: &str) -> Option<&McpTool> {
        self.tools.iter().find(|t| t.full_name() == full_name)
    }

    pub fn client(&self, server: &str) -> Option<Arc<McpClient>> {
        self.clients.iter().find(|c| c.name == server).cloned()
    }

    pub fn servers(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.clients.iter().map(|c| (c.name.clone(), "connected".to_string())).collect();
        v.extend(self.failures.iter().map(|(n, _)| (n.clone(), "failed".to_string())));
        v
    }

    pub fn stop(&self) {
        for c in &self.clients {
            c.stop();
        }
    }
}

/// 设置页：存着的 MCP 服务器。
#[tauri::command]
pub async fn mcp_servers_get(state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<McpServerConfig>, String> {
    let store = state.store.lock().map_err(|_| "读不到设置。")?;
    load(&store).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_servers_set(servers: Vec<McpServerConfig>, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<McpServerConfig>, String> {
    let store = state.store.lock().map_err(|_| "存不了设置。")?;
    save(&store, servers)
}

/// 设置页：把一台服务器的一个环境变量存进钥匙串（`value` 空着是从钥匙串删掉、不再带）。
#[tauri::command]
pub async fn mcp_secret_set(server: String, name: String, value: Option<String>, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<McpServerConfig>, String> {
    let name = name.trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("环境变量名不对：{name}"));
    }
    let mut list = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        load(&store).map_err(|e| e.to_string())?
    };
    let Some(s) = list.iter_mut().find(|s| s.name == server) else {
        return Err(format!("没有叫 {server} 的 MCP 服务器。"));
    };
    let value = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    {
        let mut keys = state.api.keys.lock().map_err(|_| "读不到钥匙串。")?;
        keys.set(&secret_id(&server, &name), value.clone())?;
    }
    if value.is_some() {
        s.secret_env.push(name.clone());
    } else {
        s.secret_env.retain(|n| n != &name);
    }
    let store = state.store.lock().map_err(|_| "存不了设置。")?;
    save(&store, list)
}

/// 设置页的 TEST：起一次、列工具、停掉（在家目录里起），顺手记下工具标注。
#[tauri::command]
pub async fn mcp_server_test(server: McpServerConfig, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<McpTool>, String> {
    let server = normalized(vec![server])?.remove(0);
    let (mut filled, missing) = with_secrets(vec![server], &state.api.keys);
    if let Some(m) = missing.first() {
        return Err(format!("钥匙串里取不到 {m}。"));
    }
    let server = filled.remove(0);
    let env = crate::term::user_env().clone().unwrap_or_else(|| std::env::vars().collect());
    let home = std::env::var("HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let client = McpClient::start(&server, &env, &home).await?;
    let tools = client.list_tools().await;
    client.stop();
    if let (Ok(t), Ok(store)) = (&tools, state.store.lock()) {
        hints_record(&store, "*", &server.name, t);
    }
    tools
}

/// 项目设置里的一行 MCP（v0.10 A2）：设置里的和项目 `.mcp.json` 里的放在一起列。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcpRow {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// `project`：项目的 `.mcp.json`；`settings`：设置 → MCP。
    pub source: &'static str,
    /// 这个项目里起不起（设置里关着的、项目里关掉的都是 false）。
    pub on: bool,
    /// 设置里关着（项目里开不了，去设置里开）。
    pub off_in_settings: bool,
    /// 被项目里同名的盖过了（设置里的那一台）。
    pub overridden: bool,
    /// 项目里的：现在这份配置点过「总是允许」。
    pub allowed: bool,
    /// 记过的工具标注：（工具，只读 / 会改 / 会删改）。
    pub tools: Vec<(String, ToolHint)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMcpView {
    pub rows: Vec<ProjectMcpRow>,
    pub skipped: Vec<(String, String)>,
    pub error: Option<String>,
}

fn project_env(ws: &crate::workspace::Workspace) -> Vec<(String, String)> {
    let base: Vec<(String, String)> = crate::term::user_env().clone().unwrap_or_else(|| std::env::vars().collect());
    crate::harness::run::with_project_env(base, &ws.settings.env_pairs())
}

/// 项目设置：这个项目的 Peck Loop 会用到的 MCP 服务器。
#[tauri::command]
pub async fn project_mcp_list(workspace_id: String, state: tauri::State<'_, crate::state::AppState>) -> Result<ProjectMcpView, String> {
    let store = state.store.lock().map_err(|_| "读不到设置。")?;
    let ws = store.workspace(&workspace_id).map_err(|_| "没找到这个项目。")?;
    let global = load(&store).map_err(|e| e.to_string())?;
    let project = read_project(Path::new(&ws.path), &project_env(&ws));
    let allowed = store.harness_allowed(&workspace_id).unwrap_or_default();
    let hints = hints_for(&store, &workspace_id);
    let off = &ws.settings.mcp_off;
    let tools_of = |name: &str| hints.get(name).map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default();
    let mut rows: Vec<ProjectMcpRow> = project
        .servers
        .iter()
        .map(|s| ProjectMcpRow {
            name: s.name.clone(),
            command: s.command.clone(),
            args: s.args.clone(),
            source: "project",
            on: !off.contains(&s.name),
            off_in_settings: false,
            overridden: false,
            allowed: s.project_rule().is_some_and(|r| allowed.contains(&r)),
            tools: tools_of(&s.name),
        })
        .collect();
    for g in &global {
        let overridden = project.servers.iter().any(|p| p.name == g.name);
        rows.push(ProjectMcpRow {
            name: g.name.clone(),
            command: g.command.clone(),
            args: g.args.clone(),
            source: "settings",
            on: g.enabled && !overridden && !off.contains(&g.name),
            off_in_settings: !g.enabled,
            overridden,
            allowed: false,
            tools: if overridden { vec![] } else { tools_of(&g.name) },
        });
    }
    Ok(ProjectMcpView { rows, skipped: project.skipped, error: project.error })
}

/// 项目设置里的 TEST：在项目文件夹里起一次、列工具、停掉，记下标注（项目里的按项目记）。
#[tauri::command]
pub async fn project_mcp_test(workspace_id: String, name: String, state: tauri::State<'_, crate::state::AppState>) -> Result<Vec<McpTool>, String> {
    let (ws, global) = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        (store.workspace(&workspace_id).map_err(|_| "没找到这个项目。")?, load(&store).map_err(|e| e.to_string())?)
    };
    let env = project_env(&ws);
    let project = read_project(Path::new(&ws.path), &env);
    let server = project.servers.iter().chain(global.iter()).find(|s| s.name == name).cloned().ok_or_else(|| format!("没有叫 {name} 的 MCP 服务器。"))?;
    let scope = if server.project.is_some() { workspace_id.clone() } else { "*".to_string() };
    let (mut filled, missing) = with_secrets(vec![server], &state.api.keys);
    if let Some(m) = missing.first() {
        return Err(format!("钥匙串里取不到 {m}。"));
    }
    let server = filled.remove(0);
    let client = McpClient::start(&server, &env, Path::new(&ws.path)).await?;
    let tools = client.list_tools().await;
    client.stop();
    if let (Ok(t), Ok(store)) = (&tools, state.store.lock()) {
        hints_record(&store, &scope, &server.name, t);
    }
    tools
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 假的 MCP 服务器：一个 sh 脚本，认 initialize、tools/list、tools/call。
    pub fn fake_server(dir: &Path) -> McpServerConfig {
        std::fs::create_dir_all(dir).unwrap();
        let script = dir.join("fake-mcp.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echo text back","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}},"annotations":{"readOnlyHint":true}},{"name":"wipe","description":"Wipe everything","inputSchema":{"type":"object"},"annotations":{"destructiveHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*) text=$(printf '%s' "$line" | sed -n 's/.*"text":"\([^"]*\)".*/\1/p'); printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"echo: %s %s"}]}}\n' "$id" "$text" "$FAKE_FLAVOR" ;;
  esac
done
"#,
        )
        .unwrap();
        McpServerConfig::new("fake", "/bin/sh", vec![script.to_string_lossy().into_owned()], vec![EnvVar { name: "FAKE_FLAVOR".into(), value: "mint".into() }])
    }

    #[test]
    fn the_project_mcp_json_is_read_like_claude_code_does() {
        let dir = std::env::temp_dir().join(format!("peck-pmcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |text: &str| std::fs::write(dir.join(".mcp.json"), text).unwrap();
        write(r#"{"mcpServers": {
            "DW": {"command": "${TOOLS:-/opt}/dw", "args": ["serve", "--root", "${ROOT}"], "env": {"TOKEN": "${SECRET}", "MODE": "x"}},
            "web": {"type": "http", "url": "https://example.test/mcp"},
            "dw": {"command": "other"}
        }}"#);
        let env = vec![("ROOT".to_string(), "/p".to_string()), ("SECRET".to_string(), "s3".to_string())];
        let got = read_project(&dir, &env);
        assert_eq!(got.servers.len(), 1, "{got:?}");
        let dw = &got.servers[0];
        assert_eq!((dw.name.as_str(), dw.command.as_str()), ("dw", "/opt/dw"));
        assert_eq!(dw.args, vec!["serve", "--root", "/p"]);
        assert!(dw.env.contains(&EnvVar { name: "TOKEN".into(), value: "s3".into() }));
        let fp = dw.project.clone().unwrap();
        assert_eq!(fp.len(), 32, "完整的 md5");
        // 给站主看的原文不展开（展开的值可能是 Token，卡片进事件日志）。
        assert_eq!(dw.raw, Some(("${TOOLS:-/opt}/dw".to_string(), vec!["serve".to_string(), "--root".to_string(), "${ROOT}".to_string()])));
        assert_eq!(got.skipped.len(), 2, "url 那种、重名的跳过：{:?}", got.skipped);
        // 换了写法，指纹跟着变（「总是允许」要再问）；只是环境变量的值变了不变。
        write(r#"{"mcpServers": {"DW": {"command": "${TOOLS:-/opt}/dw", "args": ["serve"]}}}"#);
        assert_ne!(read_project(&dir, &env).servers[0].project.as_ref(), Some(&fp));
        write(r#"{"mcpServers": {"#);
        assert!(read_project(&dir, &env).error.is_some());
        // 合并：项目的盖过设置里同名的；项目设置关掉的不起；设置里关着的不起。
        let global = vec![McpServerConfig::new("dw", "old", vec![], vec![]), McpServerConfig::new("fs", "fs", vec![], vec![]), McpServerConfig { enabled: false, ..McpServerConfig::new("off", "x", vec![], vec![]) }];
        let mut project = McpServerConfig::new("dw", "new", vec![], vec![]);
        project.project = Some("1".into());
        let m = merged(&global, &[project.clone()], &[]);
        assert_eq!(m.iter().map(|s| (s.name.as_str(), s.command.as_str())).collect::<Vec<_>>(), vec![("fs", "fs"), ("dw", "new")]);
        assert_eq!(merged(&global, &[project], &["dw".into()]).len(), 1);
        assert_eq!(expand_vars("a${X}b${Y:-d}${}c$", &|k| (k == "X").then(|| "1".to_string())), "a1bd${}c$");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tool_hints_are_kept_per_project_over_settings() {
        let dir = std::env::temp_dir().join(format!("peck-hints-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("peck.db")).unwrap();
        let tool = |name: &str, read_only: bool, destructive: bool| McpTool { server: "dw".into(), name: name.into(), description: String::new(), input_schema: json!({}), read_only, destructive };
        hints_record(&store, "*", "dw", &[tool("list", true, false), tool("wipe", false, true)]);
        hints_record(&store, "ws1", "dw", &[tool("list", true, false), tool("write_board", false, false)]);
        let p = hints_for(&store, "ws1");
        assert_eq!(p["dw"].get("write_board"), Some(&ToolHint::Write));
        assert_eq!(p["dw"].get("wipe"), None, "项目里的整台盖过设置里的");
        assert_eq!(hints_for(&store, "ws2")["dw"].get("wipe"), Some(&ToolHint::Destructive));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_in_results_are_kept_for_the_model() {
        let png = "iVBORw0KGgo=";
        let r = McpResult::from_response(&json!({"content": [
            {"type": "text", "text": "通过"},
            {"type": "image", "mimeType": "image/png", "data": png},
            {"type": "image", "mimeType": "image/tiff", "data": png},
        ]}));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].media_type, "image/png");
        assert!(r.text.starts_with("通过\n[图片 image/png，附在这一轮工具结果后面]"));
        assert!(r.text.contains("image/tiff："));
        assert!(!r.is_error);
    }

    #[tokio::test]
    async fn a_stdio_server_starts_lists_and_calls() {
        let dir = std::env::temp_dir().join(format!("peck-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = fake_server(&dir);
        let env: Vec<(String, String)> = vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())];
        let hub = McpHub::start(&[config.clone()], &env, &dir).await;
        assert!(hub.failures.is_empty(), "{:?}", hub.failures);
        assert_eq!(hub.tools.len(), 2);
        let echo = hub.find("mcp__fake__echo").unwrap();
        assert!(echo.read_only && !echo.destructive);
        assert!(hub.find("mcp__fake__wipe").unwrap().destructive);
        let r = hub.client("fake").unwrap().call("echo", json!({"text": "hi"})).await.unwrap();
        assert_eq!(r.text, "echo: hi mint");
        assert!(!r.is_error && r.images.is_empty());
        hub.stop();
        // 起不来的不挡别的，记下原因。
        let broken = McpServerConfig::new("nope", &dir.join("missing").to_string_lossy(), vec![], vec![]);
        let hub = McpHub::start(&[broken, config], &env, &dir).await;
        assert_eq!(hub.failures.len(), 1);
        assert_eq!(hub.tools.len(), 2);
        hub.stop();
        assert!(normalized(vec![McpServerConfig::new("Bad Name", "x", vec![], vec![])]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
