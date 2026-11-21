use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::mode::app_data_dir;

const DEFAULT_MUSE_TOML: &str = include_str!("../defaults/agents/muse.toml");
const DEFAULT_GROK_TOML: &str = include_str!("../defaults/agents/grok.toml");

/// Raw TOML representation of an Agent manifest (`agents/*.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_cwd_template")]
    pub cwd: String,
    #[serde(default = "default_stdin_mode")]
    pub stdin: String, // "none" | "prompt"
    #[serde(default = "default_timeout_str")]
    pub timeout: String,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_cwd_template() -> String {
    "{{workspace}}".to_string()
}

fn default_stdin_mode() -> String {
    "none".to_string()
}

fn default_timeout_str() -> String {
    "30m".to_string()
}

impl AgentManifest {
    /// Parse an `AgentManifest` from a TOML string.
    pub fn from_toml_str(raw: &str) -> Result<Self, String> {
        toml::from_str::<Self>(raw).map_err(|e| format!("Failed to parse agent TOML: {e}"))
    }
}

/// Expand leading `~/` to the user's `$HOME` directory.
pub fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(rest) = path_str.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path_str)
}

/// Build an augmented `PATH` string combining:
/// 1. Custom `path` entries from the Agent manifest
/// 2. Standard macOS CLI & toolchain locations (`~/Library/pnpm/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`)
/// 3. Existing `PATH` environment variable
pub fn build_augmented_path(extra_paths: &[String]) -> String {
    let mut dirs: Vec<PathBuf> = Vec::new();

    for p in extra_paths {
        let expanded = expand_tilde(p);
        if !dirs.contains(&expanded) {
            dirs.push(expanded);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        let candidates = [
            home_path.join("Library/pnpm/bin"),
            home_path.join(".cargo/bin"),
            home_path.join(".local/bin"),
            home_path.join(".grok/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ];
        for c in candidates {
            if !dirs.contains(&c) {
                dirs.push(c);
            }
        }
    } else {
        for c in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            let pb = PathBuf::from(c);
            if !dirs.contains(&pb) {
                dirs.push(pb);
            }
        }
    }

    if let Ok(existing_path) = std::env::var("PATH") {
        for entry in std::env::split_paths(&existing_path) {
            if !entry.as_os_str().is_empty() && !dirs.contains(&entry) {
                dirs.push(entry);
            }
        }
    }

    std::env::join_paths(dirs)
        .ok()
        .and_then(|os| os.into_string().ok())
        .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".to_string())
}

/// Ensure default `agents/*.toml` files exist in `<app_data_dir>/agents/` and load all manifests.
/// Falls back cleanly to built-in defaults if filesystem write is restricted.
pub fn load_agent_manifests() -> Vec<AgentManifest> {
    let agents_dir = app_data_dir().join("agents");
    let _ = seed_default_agents(&agents_dir);

    if let Ok(loaded) = load_manifests_from_dir(&agents_dir) {
        if !loaded.is_empty() {
            return loaded;
        }
    }

    builtin_default_manifests()
}

pub fn builtin_default_manifests() -> Vec<AgentManifest> {
    [DEFAULT_MUSE_TOML, DEFAULT_GROK_TOML]
        .iter()
        .filter_map(|raw| AgentManifest::from_toml_str(raw).ok())
        .collect()
}

fn seed_default_agents(agents_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(agents_dir)?;
    let defaults = [
        ("muse.toml", DEFAULT_MUSE_TOML),
        ("grok.toml", DEFAULT_GROK_TOML),
    ];
    for (filename, content) in defaults {
        let target = agents_dir.join(filename);
        if !target.exists() {
            fs::write(target, content)?;
        } else {
            let raw = fs::read_to_string(&target)?;
            if let Ok(mut manifest) = AgentManifest::from_toml_str(&raw) {
                let legacy_args = match filename {
                    "muse.toml" => vec![
                        "generate",
                        "--prompt",
                        "{{prompt}}",
                        "--handoff",
                        "{{handoff}}",
                    ],
                    "grok.toml" => vec![
                        "build",
                        "--context",
                        "{{handoff}}/intent.md",
                        "--tasks",
                        ".peck/tasks.md",
                    ],
                    _ => vec![],
                };
                let default = AgentManifest::from_toml_str(content).expect("built-in manifest");
                if manifest.command == default.command && manifest.args == legacy_args {
                    manifest.args = default.args;
                    if manifest.name == "Muse Wonder" {
                        manifest.name = default.name;
                    }
                    let updated =
                        toml::to_string_pretty(&manifest).map_err(std::io::Error::other)?;
                    fs::write(target, updated)?;
                }
            }
        }
    }
    Ok(())
}

pub fn load_manifests_from_dir(dir: &Path) -> Result<Vec<AgentManifest>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read agents dir: {e}"))?;
    let mut manifests = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Ok(raw) = fs::read_to_string(&path) {
                if let Ok(manifest) = AgentManifest::from_toml_str(&raw) {
                    manifests.push(manifest);
                }
            }
        }
    }

    // Sort with muse, grok, mock order first, then alphabetically
    manifests.sort_by_key(|m| match m.id.as_str() {
        "muse" => (0, m.id.clone()),
        "grok" => (1, m.id.clone()),
        "mock" => (2, m.id.clone()),
        _ => (3, m.id.clone()),
    });

    Ok(manifests)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolve a binary name or path against an augmented `PATH` string (equivalent to `which`).
pub async fn resolve_command_path(command: &str, augmented_path: &str) -> Option<PathBuf> {
    let cmd_path = expand_tilde(command);
    if cmd_path.components().count() > 1 {
        if is_executable(&cmd_path) {
            return Some(cmd_path);
        }
        return None;
    }

    // First search directly across split_paths(augmented_path)
    for dir in std::env::split_paths(augmented_path) {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    // Fallback: invoke `/usr/bin/which` with the augmented PATH
    let output = timeout(
        Duration::from_secs(2),
        Command::new("/usr/bin/which")
            .arg(command)
            .env("PATH", augmented_path)
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout.is_empty() {
            let pb = PathBuf::from(stdout.lines().next()?);
            if is_executable(&pb) {
                return Some(pb);
            }
        }
    }

    None
}

/// Probe `--version` on a resolved executable with a short 2-second timeout.
pub async fn probe_command_version(
    resolved_path: &Path,
    augmented_path: &str,
    env: &HashMap<String, String>,
) -> Result<String, String> {
    let output = timeout(
        Duration::from_secs(2),
        Command::new(resolved_path)
            .arg("--version")
            .env("PATH", augmented_path)
            .envs(env)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "--version 超时（2 秒）".to_string())?
    .map_err(|e| format!("无法执行 --version：{e}"))?;

    let combined = if !output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };

    let line = combined
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|s| s.to_string());
    if !output.status.success() {
        return Err(format!(
            "--version 退出 {}：{}",
            output.status,
            line.as_deref().unwrap_or("没有输出")
        ));
    }
    line.ok_or_else(|| "--version 没有输出".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_builtin_agent_manifests() {
        let manifests = builtin_default_manifests();
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].id, "muse");
        assert_eq!(manifests[1].id, "grok");
    }

    #[test]
    fn legacy_templates_are_updated_without_losing_custom_environment() {
        let dir = std::env::temp_dir().join(format!("peck-agent-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut legacy = AgentManifest::from_toml_str(DEFAULT_GROK_TOML).unwrap();
        legacy.args = [
            "build",
            "--context",
            "{{handoff}}/intent.md",
            "--tasks",
            ".peck/tasks.md",
        ]
        .map(String::from)
        .to_vec();
        legacy.env.insert("CUSTOM_FLAG".into(), "retained".into());
        std::fs::write(dir.join("grok.toml"), toml::to_string(&legacy).unwrap()).unwrap();
        seed_default_agents(&dir).unwrap();
        let updated = load_manifests_from_dir(&dir).unwrap();
        let grok = updated.iter().find(|a| a.id == "grok").unwrap();
        assert_eq!(grok.args[0], "-p");
        assert_eq!(grok.env["CUSTOM_FLAG"], "retained");
        let mut custom = grok.clone();
        custom.args = vec!["custom-entry".into()];
        std::fs::write(dir.join("grok.toml"), toml::to_string(&custom).unwrap()).unwrap();
        seed_default_agents(&dir).unwrap();
        let reloaded = load_manifests_from_dir(&dir).unwrap();
        assert_eq!(
            reloaded.iter().find(|a| a.id == "grok").unwrap().args[0],
            "custom-entry"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
