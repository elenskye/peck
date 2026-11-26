//! 找 CLI：展开 `~`、拼出找命令用的 PATH、按 PATH 找到可执行文件、读 `--version`。
//! v1.0.3 删了旧的 Agent 清单（数据目录的 `agents/*.toml`）：找 CLI 只看设置里的「CLI 目录」和站主 shell 的 PATH。
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

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
/// 1. Extra directories (the CLI directory, the user's shell `PATH`)
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

pub(crate) fn is_executable(path: &Path) -> bool {
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
