//! CLI 读没读到项目里的 MCP（v0.9 Phase 13，roadmap A1）。
//!
//! Peck 以项目文件夹为工作目录起官方 CLI，项目里写好的 MCP 配置（`.mcp.json`；Codex 还有 `.codex/config.toml`）
//! 由 CLI 自己读。可各家都要站主先在它自己的客户端里点一次「信任」或「批准」，没点就悄悄不连。
//! 任务开头查一下：项目里配了、这家却没读到的，写一行。信任这件事 v1.1.8 起由 TRUST 卡片管（`trust.rs`）：
//! 卡片问过、或者站主批过这个项目，就不再写这一行。
//!
//! - Codex：`codex mcp list --json`；Grok：`grok inspect --json`（带 `projectTrusted`）；Muse：`muse mcp list --json`。
//!   都不调模型，几十毫秒。
//! - Claude Code：不另起进程，看它启动时报的 `mcp_servers`（[`claude_code_missing`]）。

use super::engines::EngineSetup;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

/// 项目根上配的 MCP 服务器名：`.mcp.json` 的 `mcpServers`，Codex 再加 `.codex/config.toml` 的 `[mcp_servers.*]`。
pub fn project_servers(root: &Path, engine: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(root.join(".mcp.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(m) = v.get("mcpServers").and_then(Value::as_object) {
                out.extend(m.keys().cloned());
            }
        }
    }
    if engine == "codex" {
        if let Ok(text) = std::fs::read_to_string(root.join(".codex/config.toml")) {
            if let Ok(v) = text.parse::<toml::Table>() {
                if let Some(m) = v.get("mcp_servers").and_then(|m| m.as_table()) {
                    out.extend(m.keys().cloned());
                }
            }
        }
    }
    out
}

/// 这家 CLI 自己说读到了哪些（`None`：查不了或查不出来，不提醒）。
fn loaded(engine: &str, out: &Value) -> Option<BTreeSet<String>> {
    let names = |list: Option<&Vec<Value>>| -> BTreeSet<String> {
        list.into_iter()
            .flatten()
            .filter(|s| s.get("enabled").and_then(Value::as_bool).unwrap_or(true))
            .filter_map(|s| s.get("name").and_then(Value::as_str).map(str::to_string))
            .collect()
    };
    match engine {
        "codex" => Some(names(out.as_array())),
        "grok" => {
            if out.get("projectTrusted").and_then(Value::as_bool) == Some(false) {
                return Some(BTreeSet::new());
            }
            Some(names(out.get("mcpServers").and_then(Value::as_array)))
        }
        "muse" => Some(names(out.get("servers").and_then(Value::as_array))),
        _ => None,
    }
}

fn probe_args(engine: &str) -> Option<&'static [&'static str]> {
    match engine {
        "codex" => Some(&["mcp", "list", "--json"]),
        "grok" => Some(&["inspect", "--json"]),
        "muse" => Some(&["mcp", "list", "--json"]),
        _ => None,
    }
}

/// 哪家、去哪点（提醒那一行的后半句）。
fn how_to(engine: &str) -> &'static str {
    match engine {
        "codex" => "在这个文件夹里开一次 Codex，问要不要信任时点同意",
        "grok" => "在这个文件夹里开一次 grok，点信任（或 /hooks-trust）",
        "muse" => "在这个文件夹里开一次 muse，选「Trust and continue」",
        "claude-code" => "在这个文件夹里开一次 Claude Code，批准 .mcp.json 里的服务器",
        _ => "在它自己的客户端里点一次信任",
    }
}

/// 提醒的那一行。
pub fn notice(label: &str, engine: &str, missing: &BTreeSet<String>) -> String {
    let names = missing.iter().cloned().collect::<Vec<_>>().join("、");
    format!("{label} 没读到这个项目配的 MCP：{names}。{}。", how_to(engine))
}

/// Codex、Grok、Muse：任务开头查一次。项目里没配 MCP、查不了、都读到了，都是 `None`。
pub async fn check(engine: &str, label: &str, setup: &EngineSetup, cwd: &Path) -> Option<String> {
    let args = probe_args(engine)?;
    let expected = project_servers(cwd, engine);
    if expected.is_empty() {
        return None;
    }
    let mut cmd = tokio::process::Command::new(&setup.bin);
    cmd.args(args).current_dir(cwd).env("PATH", &setup.path).kill_on_drop(true);
    for (k, v) in &setup.env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let output = tokio::time::timeout(Duration::from_secs(8), cmd.output()).await.ok()?.ok()?;
    let parsed: Value = serde_json::from_slice(&output.stdout).ok()?;
    let got = loaded(engine, &parsed)?;
    let missing: BTreeSet<String> = expected.difference(&got).cloned().collect();
    (!missing.is_empty()).then(|| notice(label, engine, &missing))
}

/// Muse 信没信任这个文件夹（读它自己记的 `~/.config/muse/trust.json`，只读）：`None` 是读不到、不知道。
/// Muse 在没信任的工作区里连 Peck 挂在会话上的 MCP 也不连（2026-11-16 用本地假 MCP 核实过），
/// 所以提 Skill、委派这些 Peck 给它的工具也用不了。
pub fn muse_trusted(cwd: &Path) -> Option<bool> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let text = std::fs::read_to_string(home.join(".config/muse/trust.json")).ok()?;
    muse_trusted_in(&text, cwd)
}

fn muse_trusted_in(text: &str, cwd: &Path) -> Option<bool> {
    let v: Value = serde_json::from_str(text).ok()?;
    let projects = v.get("projects")?.as_object()?;
    Some(projects.iter().any(|(path, d)| d.get("decision").and_then(Value::as_str) == Some("trusted") && cwd.starts_with(path)))
}

/// Claude Code：启动时报的 `mcp_servers` 里，项目 `.mcp.json` 配了、却没读进来的（根本没列出来，或者被关掉了）。
/// 启动那一刻还在连的是 `pending`（Phase 13 真跑：批准过的 `dw` 也是 pending，过一会儿就能用），不算没读到；
/// 连失败的是服务器自己的问题，也不在这里提醒「去批准」。
pub fn claude_code_missing(cwd: &Path, reported: &[(String, String)]) -> BTreeSet<String> {
    project_servers(cwd, "claude-code")
        .into_iter()
        .filter(|name| !reported.iter().any(|(n, status)| n == name && status != "disabled"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn muse_trust_is_read_from_its_own_list() {
        let text = r#"{"schema_version":1,"projects":{"/a/proj":{"decision":"trusted"},"/b":{"decision":"denied"}}}"#;
        assert_eq!(muse_trusted_in(text, Path::new("/a/proj")), Some(true));
        assert_eq!(muse_trusted_in(text, Path::new("/a/proj/sub")), Some(true));
        assert_eq!(muse_trusted_in(text, Path::new("/b")), Some(false));
        assert_eq!(muse_trusted_in(text, Path::new("/c")), Some(false));
        assert_eq!(muse_trusted_in("坏的", Path::new("/c")), None);
    }

    fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("peck-project-mcp-{}", uuid::Uuid::new_v4()));
        for (path, text) in files {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_project_config_each_engine_uses() {
        let dir = project(&[
            (".mcp.json", r#"{"mcpServers": {"dw": {"command": "bench/bin/dw-mcp.sh"}}}"#),
            (".codex/config.toml", "[mcp_servers.dw]\ncommand = \"/bin/sh\"\n[mcp_servers.extra]\ncommand = \"x\"\n"),
        ]);
        assert_eq!(project_servers(&dir, "grok").into_iter().collect::<Vec<_>>(), vec!["dw"]);
        assert_eq!(project_servers(&dir, "codex").into_iter().collect::<Vec<_>>(), vec!["dw", "extra"]);
        assert!(project_servers(&project(&[]), "muse").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_each_cli_says_it_loaded() {
        let dw: BTreeSet<String> = ["dw".to_string()].into();
        // Codex：列表里关掉的不算。
        assert_eq!(loaded("codex", &json!([{"name": "dw", "enabled": true}, {"name": "x", "enabled": false}])), Some(dw.clone()));
        // Grok：文件夹没信任就是什么都没读。
        assert_eq!(loaded("grok", &json!({"projectTrusted": true, "mcpServers": [{"name": "dw"}]})), Some(dw.clone()));
        assert_eq!(loaded("grok", &json!({"projectTrusted": false, "mcpServers": [{"name": "dw"}]})), Some(BTreeSet::new()));
        // Muse：没信任时列表是空的。
        assert_eq!(loaded("muse", &json!({"servers": []})), Some(BTreeSet::new()));
        assert_eq!(loaded("nope", &json!({})), None);
        let line = notice("Muse", "muse", &dw);
        assert!(line.starts_with("Muse 没读到这个项目配的 MCP：dw。") && line.ends_with("选「Trust and continue」。"));
    }

    #[test]
    fn claude_code_counts_listed_servers() {
        let dir = project(&[(".mcp.json", r#"{"mcpServers": {"dw": {}, "db": {}}}"#)]);
        // 在连的、连上的都算读到了；没列出来的、关掉的才提醒。
        let reported = vec![("dw".to_string(), "pending".to_string())];
        assert_eq!(claude_code_missing(&dir, &reported).into_iter().collect::<Vec<_>>(), vec!["db"]);
        let reported = vec![("dw".to_string(), "connected".to_string()), ("db".to_string(), "disabled".to_string())];
        assert_eq!(claude_code_missing(&dir, &reported).into_iter().collect::<Vec<_>>(), vec!["db"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
