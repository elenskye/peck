//! 权限三档在 Peck 循环里怎么管：
//!
//! | 档 | 工具 | 命令 |
//! | --- | --- | --- |
//! | PLAN | 只给读的工具、`update_plan`、`ask_user` | 只读沙箱 |
//! | EDIT（默认） | 全部 | 装依赖、起常驻进程前问一次（能选「这个项目总是允许」） |
//! | AUTO | 全部 | 除了高危都不问 |
//!
//! 高危（`risk.rs`）不管哪档都弹卡片，只有「批准」「不行」，不能总是允许。

use super::risk::{self, Risk};
use super::tools::bash::Sandbox;
use crate::gateway::event::PermissionLevel;
use std::collections::HashSet;
use std::path::Path;

/// EDIT 档要问一次的两类命令（「总是允许」按项目记）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confirm {
    InstallDeps,
    LongRunning,
}

impl Confirm {
    /// 存「总是允许」用的名字。
    pub fn rule(self) -> &'static str {
        match self {
            Self::InstallDeps => "install-deps",
            Self::LongRunning => "long-running",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::InstallDeps => "装依赖",
            Self::LongRunning => "起常驻进程",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// 直接跑。
    No,
    /// 高危：弹卡片，只有批准、不行。
    HighRisk(Vec<Risk>),
    /// 不是高危，但这一档要问一次；能选「这个项目总是允许」。
    Confirm(Confirm),
}

pub fn sandbox_for(level: PermissionLevel) -> Sandbox {
    match level {
        PermissionLevel::Plan => Sandbox::ReadOnly,
        PermissionLevel::Edit | PermissionLevel::Auto => Sandbox::Project,
    }
}

fn installs(cmd: &str, args: &[String]) -> bool {
    let sub = args.iter().find(|a| !a.starts_with('-')).map(String::as_str).unwrap_or("");
    match cmd {
        "npm" | "pnpm" | "yarn" | "bun" | "cnpm" => {
            (cmd == "yarn" && args.is_empty())
                || matches!(sub, "install" | "i" | "add" | "ci" | "update" | "up" | "upgrade")
        }
        "cargo" => matches!(sub, "add" | "install"),
        "pip" | "pip3" | "pipx" | "gem" | "conda" | "mamba" | "apt" | "apt-get" | "port" => matches!(sub, "install"),
        "uv" => matches!(sub, "add" | "pip" | "tool") && args.iter().any(|a| a == "install" || a == "add" || sub == "add"),
        "python" | "python3" => args.windows(3).any(|w| w[0] == "-m" && w[1] == "pip" && w[2] == "install"),
        "poetry" => matches!(sub, "add" | "install"),
        "brew" => matches!(sub, "install" | "reinstall" | "upgrade" | "tap"),
        "go" => matches!(sub, "get" | "install"),
        "composer" => matches!(sub, "require" | "install"),
        "bundle" => matches!(sub, "install" | "add"),
        "deno" => matches!(sub, "add" | "install"),
        _ => false,
    }
}

fn long_running(cmd: &str, args: &[String], segment: &str) -> bool {
    let sub = args.iter().find(|a| !a.starts_with('-')).map(String::as_str).unwrap_or("");
    let script_of = |a: &[String]| -> String {
        // `npm run dev`、`pnpm dev`
        if a.first().is_some_and(|f| f == "run" || f == "run-script") {
            a.get(1).cloned().unwrap_or_default()
        } else {
            a.first().cloned().unwrap_or_default()
        }
    };
    let watchy = |s: &str| matches!(s, "dev" | "start" | "serve" | "watch" | "preview") || s.starts_with("dev:") || s.ends_with(":watch");
    if args.iter().any(|a| a == "--watch" || a == "watch") && cmd != "git" {
        return true;
    }
    match cmd {
        "npm" | "pnpm" | "yarn" | "bun" => watchy(&script_of(args)),
        "vite" => !matches!(sub, "build" | "optimize"),
        "next" | "nuxt" | "astro" | "remix" | "svelte-kit" => matches!(sub, "dev" | "start" | "preview"),
        "webpack" | "webpack-dev-server" => sub == "serve" || cmd == "webpack-dev-server",
        "cargo" => matches!(sub, "watch" | "leptos") || (sub == "tauri" && args.iter().any(|a| a == "dev")),
        "tauri" => sub == "dev",
        "python" | "python3" => args.windows(2).any(|w| w[0] == "-m" && matches!(w[1].as_str(), "http.server" | "uvicorn" | "flask")),
        "flask" => sub == "run",
        "uvicorn" | "gunicorn" | "nodemon" | "http-server" | "live-server" | "serve" | "hugo" => cmd != "hugo" || sub == "server",
        "rails" => matches!(sub, "s" | "server"),
        "php" => args.iter().any(|a| a == "-S"),
        "tail" => args.iter().any(|a| a == "-f" || a == "-F"),
        "docker" | "docker-compose" => (sub == "compose" && args.iter().any(|a| a == "up") || sub == "up") && !args.iter().any(|a| a == "-d" || a == "--detach"),
        _ => segment.trim_end().ends_with('&') && !segment.trim_end().ends_with("&&"),
    }
}

/// 这条命令属于哪些「EDIT 要问一次」的类。
pub fn confirms(command: &str) -> Vec<Confirm> {
    let mut out = Vec::new();
    // 末尾的 `&`（放后台）在拆段时会被吃掉，先看一眼原文。
    let backgrounded = {
        let t = command.trim_end();
        t.ends_with('&') && !t.ends_with("&&")
    };
    for segment in risk::segments(command) {
        let w = risk::words(&segment);
        let (cmd_words, _) = risk::strip_wrappers(&w);
        let Some(first) = cmd_words.first() else { continue };
        let cmd = first.rsplit('/').next().unwrap_or(first);
        let args = &cmd_words[1..];
        if installs(cmd, args) && !out.contains(&Confirm::InstallDeps) {
            out.push(Confirm::InstallDeps);
        }
        if long_running(cmd, args, &segment) && !out.contains(&Confirm::LongRunning) {
            out.push(Confirm::LongRunning);
        }
    }
    if (backgrounded || command.split_whitespace().next() == Some("nohup")) && !out.contains(&Confirm::LongRunning) {
        out.push(Confirm::LongRunning);
    }
    out
}

/// 跑这条命令之前要不要问。`allowed` 是这个项目「总是允许」过的类。
pub fn check_command(level: PermissionLevel, command: &str, root: &Path, allowed: &HashSet<String>) -> Ask {
    let risks = risk::assess(command, root);
    if !risks.is_empty() {
        return Ask::HighRisk(risks);
    }
    if level != PermissionLevel::Edit {
        return Ask::No;
    }
    match confirms(command).into_iter().find(|c| !allowed.contains(c.rule())) {
        Some(c) => Ask::Confirm(c),
        None => Ask::No,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn edit_asks_once_for_installs_and_servers() {
        let root = Path::new("/p");
        for c in ["pnpm install", "npm i -D vitest", "yarn", "yarn add react", "cargo add serde", "pip install requests", "python3 -m pip install x", "brew install jq", "uv add httpx", "go get example.com/x"] {
            assert_eq!(check_command(PermissionLevel::Edit, c, root, &none()), Ask::Confirm(Confirm::InstallDeps), "{c}");
        }
        for c in ["pnpm dev", "npm run dev", "npm start", "vite", "next dev", "cargo watch -x run", "python3 -m http.server 8000", "tail -f log.txt", "node server.js &", "nohup node a.js", "docker compose up", "vitest --watch"] {
            assert_eq!(check_command(PermissionLevel::Edit, c, root, &none()), Ask::Confirm(Confirm::LongRunning), "{c}");
        }
        for c in ["pnpm test", "npm run build", "vite build", "cargo test", "cargo build", "pip list", "npm ls", "docker compose up -d", "ls && cat a"] {
            assert_eq!(check_command(PermissionLevel::Edit, c, root, &none()), Ask::No, "{c}");
        }
    }

    #[test]
    fn always_allow_is_per_kind_and_auto_never_asks() {
        let root = Path::new("/p");
        let allowed: HashSet<String> = [Confirm::InstallDeps.rule().to_string()].into();
        assert_eq!(check_command(PermissionLevel::Edit, "pnpm install", root, &allowed), Ask::No);
        assert_eq!(check_command(PermissionLevel::Edit, "pnpm dev", root, &allowed), Ask::Confirm(Confirm::LongRunning));
        assert_eq!(check_command(PermissionLevel::Auto, "pnpm install", root, &none()), Ask::No);
        assert_eq!(check_command(PermissionLevel::Plan, "pnpm install", root, &none()), Ask::No, "PLAN 在只读沙箱里，装不上");
    }

    #[test]
    fn high_risk_asks_in_every_level() {
        let root = Path::new("/p");
        for level in [PermissionLevel::Plan, PermissionLevel::Edit, PermissionLevel::Auto] {
            assert!(matches!(check_command(level, "rm -rf dist", root, &none()), Ask::HighRisk(_)));
            assert!(matches!(check_command(level, "git push", root, &none()), Ask::HighRisk(_)));
        }
        let allowed: HashSet<String> = ["install-deps".to_string()].into();
        assert!(matches!(check_command(PermissionLevel::Edit, "sudo npm i -g x", root, &allowed), Ask::HighRisk(_)));
        assert_eq!(sandbox_for(PermissionLevel::Plan), Sandbox::ReadOnly);
        assert_eq!(sandbox_for(PermissionLevel::Auto), Sandbox::Project);
    }
}
