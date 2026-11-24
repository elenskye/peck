//! CLI 信不信任项目文件夹（v1.1.8）。
//!
//! 各家 CLI 没信任项目文件夹时会悄悄少干事（Muse 不连 Peck 挂给它的工具，Grok、Codex 不读项目里的配置，
//! Claude Code 不连 `.mcp.json` 里没批过的服务器）。站主定的做法：任务开头弹一张 TRUST 卡片，点了以后 Peck 记在
//! 项目设置里（`trusted`），之后每次起 CLI 都带上这家官方的开关——Peck 不直接写任何 CLI 的文件：
//!
//! | CLI | 开关 | 落不落到它自己的配置 |
//! | --- | --- | --- |
//! | Muse | `muse serve --trust-workspace` | 不落，只这一次进程 |
//! | Codex | `-c projects={"<文件夹>"={trust_level="trusted"}}` | 不落（`projects."…".trust_level=` 那种写法它不认，2026-11-24 试过） |
//! | Claude Code | `--settings {"enableAllProjectMcpServers":true}` | 不落（无头模式本来不问文件夹信任，卡的是 `.mcp.json`） |
//! | Grok | `grok --trust …`（没有只这一次的） | 落：它自己记进 `~/.grok/trusted_folders.toml` |

use super::engines::EngineSetup;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// 卡片的编号：`trust-<任务 id>`。
pub fn request_id(run_id: &str) -> String {
    format!("trust-{run_id}")
}

/// 点过 NOT NOW 的（项目, 引擎）：这次打开 Peck 期间不再问。
fn skipped() -> &'static Mutex<HashSet<(String, String)>> {
    static S: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn skip(workspace_id: &str, engine: &str) {
    if let Ok(mut s) = skipped().lock() {
        s.insert((workspace_id.to_string(), engine.to_string()));
    }
}

pub fn is_skipped(workspace_id: &str, engine: &str) -> bool {
    skipped().lock().map(|s| s.contains(&(workspace_id.to_string(), engine.to_string()))).unwrap_or(false)
}

/// 这家 CLI 现在是不是没信任这个文件夹（只读地看；看不出来算信任着，不打扰）。
pub async fn untrusted(engine: &str, setup: &EngineSetup, cwd: &Path) -> bool {
    match engine {
        "muse" => super::project_mcp::muse_trusted(cwd) == Some(false),
        "grok" => grok_trusted(setup, cwd).await == Some(false),
        "codex" => cwd.join(".codex").is_dir() && codex_trusted(setup, cwd) == Some(false),
        // 无头模式不问文件夹信任；项目配了 `.mcp.json` 才有要批的。
        "claude-code" => !super::project_mcp::project_servers(cwd, engine).is_empty(),
        _ => false,
    }
}

/// 批过以后起这家 CLI 时多带的参数（放在子命令前面还是后面各家不同，见各引擎）。
pub fn launch_args(engine: &str, cwd: &Path) -> Vec<String> {
    match engine {
        "muse" => vec!["--trust-workspace".into()],
        "codex" => vec!["-c".into(), codex_override(cwd)],
        "claude-code" => vec!["--settings".into(), r#"{"enableAllProjectMcpServers":true}"#.into()],
        _ => vec![],
    }
}

/// Codex 的 `-c` 值：整个 `projects` 用内联表给（TOML 的基本字符串，路径里的 `\`、`"` 转义）。
fn codex_override(cwd: &Path) -> String {
    let path = cwd.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    format!("projects={{\"{path}\"={{trust_level=\"trusted\"}}}}")
}

/// Grok：用它自己的 `--trust` 记一次（`grok --trust mcp list`，不调模型）。记完再看一眼，没记上给原因。
pub async fn grant_grok(setup: &EngineSetup, cwd: &Path) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(&setup.bin);
    cmd.args(["--trust", "mcp", "list"]).current_dir(cwd).env("PATH", &setup.path).kill_on_drop(true);
    for (k, v) in &setup.env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let ran = tokio::time::timeout(Duration::from_secs(12), cmd.status()).await;
    if !matches!(ran, Ok(Ok(_))) {
        return Err("Grok 的 --trust 没跑起来。".into());
    }
    match grok_trusted(setup, cwd).await {
        Some(false) => Err("Grok 没记上信任。".into()),
        _ => Ok(()),
    }
}

/// `grok inspect --json` 的 `projectTrusted`（几十毫秒，不调模型）。
async fn grok_trusted(setup: &EngineSetup, cwd: &Path) -> Option<bool> {
    let mut cmd = tokio::process::Command::new(&setup.bin);
    cmd.args(["inspect", "--json"]).current_dir(cwd).env("PATH", &setup.path).kill_on_drop(true);
    for (k, v) in &setup.env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let output = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await.ok()?.ok()?;
    serde_json::from_slice::<Value>(&output.stdout).ok()?.get("projectTrusted")?.as_bool()
}

/// Codex 记没记着信任（读它的 `config.toml` 的 `[projects."…"]`，只读；这个文件夹或者上面哪一层是 trusted 就算）。
fn codex_trusted(setup: &EngineSetup, cwd: &Path) -> Option<bool> {
    let home = setup
        .env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| std::path::PathBuf::from(v))
        .or_else(|| std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".codex")))?;
    let text = std::fs::read_to_string(home.join("config.toml")).ok()?;
    codex_trusted_in(&text, cwd)
}

fn codex_trusted_in(text: &str, cwd: &Path) -> Option<bool> {
    let table = text.parse::<toml::Table>().ok()?;
    let Some(projects) = table.get("projects").and_then(|p| p.as_table()) else {
        return Some(false);
    };
    Some(projects.iter().any(|(path, v)| v.get("trust_level").and_then(|t| t.as_str()) == Some("trusted") && cwd.starts_with(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_switches_are_the_official_per_run_ones() {
        let cwd = Path::new("/Users/me/my.project \"x\"");
        assert_eq!(launch_args("muse", cwd), ["--trust-workspace"]);
        assert_eq!(launch_args("codex", cwd), ["-c", r#"projects={"/Users/me/my.project \"x\""={trust_level="trusted"}}"#]);
        // 那个值是合法的 TOML。
        let parsed = launch_args("codex", cwd)[1].parse::<toml::Table>().unwrap();
        assert_eq!(parsed["projects"]["/Users/me/my.project \"x\""]["trust_level"].as_str(), Some("trusted"));
        assert_eq!(launch_args("claude-code", cwd)[0], "--settings");
        // Grok 没有只这一次的开关（用 `grant_grok`）。
        assert!(launch_args("grok", cwd).is_empty());
    }

    #[test]
    fn codex_trust_is_read_from_its_projects_table() {
        let text = "model = \"x\"\n[projects.\"/a/b\"]\ntrust_level = \"trusted\"\n[projects.\"/c\"]\ntrust_level = \"untrusted\"\n";
        assert_eq!(codex_trusted_in(text, Path::new("/a/b")), Some(true));
        assert_eq!(codex_trusted_in(text, Path::new("/a/b/inner")), Some(true));
        assert_eq!(codex_trusted_in(text, Path::new("/c")), Some(false));
        assert_eq!(codex_trusted_in("model = \"x\"\n", Path::new("/a")), Some(false));
        assert_eq!(codex_trusted_in("not toml [", Path::new("/a")), None);
    }

    #[test]
    fn not_now_is_remembered_per_project_and_engine() {
        assert!(!is_skipped("ws-trust-test", "muse"));
        skip("ws-trust-test", "muse");
        assert!(is_skipped("ws-trust-test", "muse") && !is_skipped("ws-trust-test", "grok"));
    }
}
