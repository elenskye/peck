//! 自己挂的脚本 Hook（v1.1.2）：借鉴 Claude Code，在 Peck Loop 的几个时刻跑项目里配的命令。CLI 引擎 Peck 只替它跑
//! 它控制得了的三个时刻（v1.1.4，见 `gateway/cli_hooks.rs`）。
//!
//! - **从哪读**：项目的 `.claude/settings.json`、`.claude/settings.local.json` 里的 `hooks`，加上装进项目的插件的
//!   `hooks/hooks.json`。格式照 Claude Code：`{"hooks": {"PreToolUse": [{"matcher": "Bash|Edit", "hooks": [{"type": "command",
//!   "command": "…", "timeout": 60}]}]}}`；只认 `type: command`（插件的 `type: peck` 是收尾提交，见 `hooks.rs`）。
//! - **六个时刻**：`SessionStart`（v1.1.4：对话第一句发出去之前 `startup`、压成交接摘要之后 `compact`；`matcher` 对 source）、
//!   `UserPromptSubmit`（站主发话时）、`PreToolUse` / `PostToolUse`（调工具前后，`matcher` 按工具名的正则，
//!   Peck 的名字和 Claude Code 的叫法都认：`Bash` 对 `bash_run`）、`PreCompact`（自动压缩前）、`Stop`（主角色要收尾时）。
//! - **怎么跑**：`/bin/sh -c`，在项目根、站主 shell 的环境里（去掉像 Key 的变量），不进沙箱；JSON 从 stdin 进；默认 60 秒。
//!   每一条命令第一次跑之前弹卡片问站主（ALWAYS 按命令原文的指纹记进「总是允许」），拒了这一回合不再问；插件库自带、
//!   一字没改的（Git Plan 的 HANDOFF 检查）不问。ONE SHOT 不跑。
//! - **退出码**（和 Claude Code 一样）：0 照常（stdout 是 JSON 就读 `decision` / `reason`、`hookSpecificOutput`；
//!   `SessionStart`、`UserPromptSubmit` 的 stdout 当补充上下文）；2 拦下（stderr 是原因：调工具前就不跑这一步、调工具后把原因交给模型、
//!   发话时这句话不发、收尾时让模型接着干；`SessionStart`、`PreCompact` 拦不住，只记一笔）；别的码、超时只记一笔，照常往下。

use crate::gateway::event::PermissionLevel;
use regex::Regex;
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const DEFAULT_TIMEOUT: u64 = 60;
const MAX_TIMEOUT: u64 = 600;
/// 收尾 Hook 一回合里最多拦几次（防止脚本写错了一直不让停）。
pub const MAX_STOP_BLOCKS: u32 = 3;
/// 给模型、给界面的输出最多多长。
const OUTPUT_BYTES: usize = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PreCompact,
    Stop,
}

impl Event {
    pub const ALL: [Event; 6] = [Event::SessionStart, Event::UserPromptSubmit, Event::PreToolUse, Event::PostToolUse, Event::PreCompact, Event::Stop];

    pub fn key(self) -> &'static str {
        match self {
            Event::SessionStart => "SessionStart",
            Event::UserPromptSubmit => "UserPromptSubmit",
            Event::PreToolUse => "PreToolUse",
            Event::PostToolUse => "PostToolUse",
            Event::PreCompact => "PreCompact",
            Event::Stop => "Stop",
        }
    }

    /// 给人看的。
    pub fn word(self) -> &'static str {
        match self {
            Event::SessionStart => "开场时",
            Event::UserPromptSubmit => "发话时",
            Event::PreToolUse => "调工具前",
            Event::PostToolUse => "调工具后",
            Event::PreCompact => "压缩前",
            Event::Stop => "收尾时",
        }
    }
}

/// Peck 的工具在 Claude Code 里叫什么（matcher 两种叫法都认，Claude Code 的脚本拿来就能用）。
pub fn claude_name(tool: &str) -> Option<&'static str> {
    use super::tools::*;
    Some(match tool {
        BASH_RUN | BASH_BACKGROUND => "Bash",
        READ_FILE | READ_PDF | OCR_PDF | READ_NOTEBOOK => "Read",
        WRITE_FILE => "Write",
        WRITE_PATCH | APPLY_PATCH => "Edit",
        SEARCH_TEXT => "Grep",
        FIND_FILES => "Glob",
        LIST_DIRECTORY => "LS",
        WEB_SEARCH => "WebSearch",
        UPDATE_PLAN => "TodoWrite",
        TOOL_SEARCH => "ToolSearch",
        super::agents::SPAWN_AGENT => "Task",
        _ => return None,
    })
}

/// 一条挂好的命令。
#[derive(Debug, Clone)]
pub struct Hook {
    pub event: Event,
    /// 工具名的正则（整个名字要对上）；空的、`*` 是全部。调工具前后对工具名，`SessionStart` 对 source（`startup` / `compact`）。
    pub matcher: Option<String>,
    pub command: String,
    pub timeout_secs: u64,
    /// 从哪来的：`.claude/settings.json`、`插件 git-plan` …
    pub source: String,
    /// 插件的文件夹（命令里的 `${CLAUDE_PLUGIN_ROOT}` 换成它）。
    pub plugin_root: Option<PathBuf>,
    /// 插件库自带、一字没改的（v1.1.4，`plugins::library_hooks_intact`）：不弹卡片。
    pub trusted: bool,
}

impl Hook {
    pub fn matches(&self, tool: Option<&str>) -> bool {
        let Some(tool) = tool else { return true };
        if self.event == Event::SessionStart {
            // source 只有几个固定的词：照 Claude Code 当正则对，claude_name 那层不用。
            let pattern = match self.matcher.as_deref().map(str::trim) {
                None | Some("") | Some("*") => return true,
                Some(p) => p,
            };
            return Regex::new(&format!("^(?:{pattern})$")).map(|re| re.is_match(tool)).unwrap_or(pattern == tool);
        }
        let pattern = match self.matcher.as_deref().map(str::trim) {
            None | Some("") | Some("*") => return true,
            Some(p) => p,
        };
        let Ok(re) = Regex::new(&format!("^(?:{pattern})$")) else { return pattern == tool };
        re.is_match(tool) || claude_name(tool).is_some_and(|c| re.is_match(c))
    }

    /// 「总是允许」的规则名：按时刻、matcher、命令原文的指纹（命令改了就再问）。
    pub fn rule(&self) -> String {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.event.key(), self.matcher.as_deref().unwrap_or(""), self.command.as_str()).hash(&mut h);
        format!("hook:{:016x}", h.finish())
    }

    /// 真正跑的那条命令（插件的换上它的文件夹）。
    fn command_line(&self) -> String {
        match &self.plugin_root {
            Some(root) => self.command.replace("${CLAUDE_PLUGIN_ROOT}", &root.to_string_lossy()),
            None => self.command.clone(),
        }
    }

    /// 卡片和记录上的一行：「调工具前 · ./scripts/check.sh」。
    pub fn title(&self) -> String {
        let one: String = self.command.lines().next().unwrap_or("").chars().take(80).collect();
        format!("{} · {one}", self.event.word())
    }
}

/// 这个项目挂的全部命令。
#[derive(Debug, Clone, Default)]
pub struct HookSet {
    pub hooks: Vec<Hook>,
}

impl HookSet {
    /// 读项目的设置文件和装进来的插件。读不懂的文件跳过。
    pub fn load(root: &Path) -> Self {
        let mut hooks = Vec::new();
        for file in [".claude/settings.json", ".claude/settings.local.json"] {
            if let Some(v) = read_json(&root.join(file)) {
                hooks.extend(parse(&v, file, None));
            }
        }
        for (plugin, dir) in crate::project_plugins::installed(root) {
            if let Some(v) = read_json(&dir.join("hooks/hooks.json")) {
                let trusted = crate::plugins::library_hooks_intact(&plugin.info.name, &dir);
                hooks.extend(parse(&v, &format!("插件 {}", plugin.info.name), Some(dir.clone())).into_iter().map(|h| Hook { trusted, ..h }));
            }
        }
        Self { hooks }
    }

    /// 只留插件带的（v1.1.4：Claude Code 自己跑项目 `.claude/settings.json` 里的，Peck 只替它跑插件的，免得跑两遍）。
    pub fn only_plugins(mut self) -> Self {
        self.hooks.retain(|h| h.plugin_root.is_some());
        self
    }

    pub fn for_event(&self, event: Event, tool: Option<&str>) -> Vec<Hook> {
        self.hooks.iter().filter(|h| h.event == event && h.matches(tool)).cloned().collect()
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// `{"hooks": {"<时刻>": [{"matcher": …, "hooks": [{"type": "command", …}]}]}}` → 命令。
pub fn parse(v: &Value, source: &str, plugin_root: Option<PathBuf>) -> Vec<Hook> {
    let mut out = Vec::new();
    for event in Event::ALL {
        for group in v["hooks"][event.key()].as_array().into_iter().flatten() {
            let matcher = group["matcher"].as_str().map(str::to_string);
            for h in group["hooks"].as_array().into_iter().flatten() {
                let Some(command) = h["command"].as_str().filter(|c| !c.trim().is_empty()) else { continue };
                if h["type"].as_str().unwrap_or("command") != "command" {
                    continue;
                }
                out.push(Hook {
                    event,
                    matcher: matcher.clone(),
                    command: command.to_string(),
                    timeout_secs: h["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT),
                    source: source.to_string(),
                    plugin_root: plugin_root.clone(),
                    trusted: false,
                });
            }
        }
    }
    out
}

/// 一次跑的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    /// 起不来（找不到 sh 这类）。
    pub failed: Option<String>,
}

/// 脚本怎么说。
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// 拦下了，原因（给模型看的）。
    pub block: Option<String>,
    /// 补充给模型的话。
    pub context: Option<String>,
    /// 出错了（不是 0 也不是 2、超时、起不来）：只记一笔。
    pub error: Option<String>,
    /// 给界面看的一句：stdout / stderr 的头几行。
    pub shown: String,
}

/// 照 Claude Code 的规则读结果。
pub fn verdict(event: Event, ran: &Ran) -> Verdict {
    let clip = |s: &str| super::tools::clip_bytes(s.trim(), OUTPUT_BYTES).to_string();
    if let Some(why) = &ran.failed {
        return Verdict { block: None, context: None, error: Some(why.clone()), shown: why.clone() };
    }
    if ran.timed_out {
        return Verdict { block: None, context: None, error: Some("超时，停掉了".into()), shown: "超时，停掉了".into() };
    }
    match ran.code {
        Some(2) => {
            let why = if ran.stderr.trim().is_empty() { "（脚本没写原因）".to_string() } else { clip(&ran.stderr) };
            // 开场时拦不住（和 Claude Code 一样只给人看）：记成一笔出错，照常往下。
            if event == Event::SessionStart {
                return Verdict { block: None, context: None, error: Some(format!("退出码 2：{why}")), shown: why };
            }
            let block = (event != Event::PreCompact).then(|| why.clone());
            Verdict { block, context: None, error: None, shown: why }
        }
        Some(0) => {
            let out = ran.stdout.trim();
            let parsed = out.starts_with('{').then(|| serde_json::from_str::<Value>(out).ok()).flatten();
            match parsed {
                Some(v) => {
                    let specific = &v["hookSpecificOutput"];
                    let reason = v["reason"].as_str().or_else(|| specific["permissionDecisionReason"].as_str()).or_else(|| v["stopReason"].as_str()).map(str::to_string);
                    let blocked = v["decision"] == "block" || specific["permissionDecision"] == "deny" || v["continue"] == false;
                    let block = (blocked && !matches!(event, Event::PreCompact | Event::SessionStart)).then(|| reason.clone().unwrap_or_else(|| "（脚本没写原因）".into()));
                    let context = specific["additionalContext"].as_str().filter(|s| !s.trim().is_empty()).map(clip);
                    Verdict { shown: block.clone().or_else(|| context.clone()).unwrap_or_default(), block, context, error: None }
                }
                // 开场时、发话时的 stdout 是补充上下文（和 Claude Code 一样）；别的时刻只给人看。
                None => {
                    let context = (matches!(event, Event::SessionStart | Event::UserPromptSubmit) && !out.is_empty()).then(|| clip(out));
                    Verdict { block: None, context, error: None, shown: clip(out) }
                }
            }
        }
        code => {
            let why = format!("退出码 {}{}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()), if ran.stderr.trim().is_empty() { String::new() } else { format!("：{}", clip(&ran.stderr)) });
            Verdict { block: None, context: None, error: Some(why.clone()), shown: why }
        }
    }
}

/// 交给脚本的 JSON（stdin）：和 Claude Code 的字段一样，`tool_name` 用 Claude Code 的叫法（没有对应的用 Peck 的），
/// `peck_tool_name` 是 Peck 的。
pub fn input(event: Event, session: &str, cwd: &Path, level: PermissionLevel, extra: Value) -> Value {
    let mode = match level {
        PermissionLevel::Plan => "plan",
        PermissionLevel::Edit => "default",
        PermissionLevel::Auto => "acceptEdits",
    };
    let mut v = json!({"session_id": session, "transcript_path": null, "cwd": cwd, "hook_event_name": event.key(), "permission_mode": mode});
    if let (Some(o), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in extra {
            o.insert(k.clone(), x.clone());
        }
        if let Some(tool) = extra.get("tool_name").and_then(Value::as_str) {
            o.insert("peck_tool_name".into(), json!(tool));
            if let Some(c) = claude_name(tool) {
                o.insert("tool_name".into(), json!(c));
            }
        }
    }
    v
}

/// 跑一条：`sh -c`，在项目根，JSON 进 stdin，超时停掉整组进程。
pub async fn run(hook: &Hook, stdin: &Value, cwd: &Path, env: &[(String, String)]) -> Ran {
    let mut cmd = tokio::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(hook.command_line()).current_dir(cwd).env_clear();
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.env("CLAUDE_PROJECT_DIR", cwd).env("PECK_PROJECT_DIR", cwd).env("PECK_HOOK_EVENT", hook.event.key());
    if let Some(root) = &hook.plugin_root {
        cmd.env("CLAUDE_PLUGIN_ROOT", root);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Ran { failed: Some(format!("起不来：{e}")), ..Default::default() },
    };
    if let Some(mut pipe) = child.stdin.take() {
        let body = stdin.to_string();
        tokio::spawn(async move {
            let _ = pipe.write_all(body.as_bytes()).await;
        });
    }
    let pid = child.id();
    match tokio::time::timeout(Duration::from_secs(hook.timeout_secs), child.wait_with_output()).await {
        Ok(Ok(out)) => Ran {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            ..Default::default()
        },
        Ok(Err(e)) => Ran { failed: Some(format!("没跑完：{e}")), ..Default::default() },
        Err(_) => {
            // 整组停掉（脚本自己起的子进程也停）。
            crate::process::terminate_process_group(pid).await;
            Ran { timed_out: true, ..Default::default() }
        }
    }
}

/// 跑一条、照规矩读结果，再给回要发的 `hook.ran`（拦下的、出错的、有输出的；静默成功的没有）。Peck Loop 和 CLI 引擎共用。
pub async fn run_and_judge(hook: &Hook, input: &Value, cwd: &Path, env: &[(String, String)]) -> (Verdict, Option<crate::gateway::event::EventBody>) {
    let ran = run(hook, input, cwd, env).await;
    let v = verdict(hook.event, &ran);
    let note = (v.block.is_some() || v.error.is_some() || !v.shown.is_empty()).then(|| {
        let text = match (&v.block, &v.error) {
            (Some(why), _) => format!("拦下了：{why}"),
            (_, Some(e)) => format!("出错了：{e}"),
            _ => v.shown.clone(),
        };
        crate::gateway::event::EventBody::HookRan { plugin: hook.source.clone(), action: hook.title(), ok: v.error.is_none() && v.block.is_none(), text, event: Some(hook.event.key().into()) }
    });
    (v, note)
}

/// 补充给模型的话包一层（放在这一句前面、交接摘要后面）。
pub fn context_block(event: Event, text: &str) -> String {
    format!("<hook-context event=\"{}\">\n{text}\n</hook-context>", event.key())
}

/// 收尾时拦下了：交给模型接着干的那一句。
pub fn stop_feedback(why: &str) -> String {
    format!("<hook-feedback event=\"Stop\">\n{why}\n</hook-feedback>\nA project hook asked you to keep going before you finish. Deal with it, then finish.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(event: Event, matcher: Option<&str>, command: &str) -> Hook {
        Hook { event, matcher: matcher.map(str::to_string), command: command.into(), timeout_secs: 5, source: "test".into(), plugin_root: None, trusted: false }
    }

    #[test]
    fn settings_parse_and_matchers_know_both_names() {
        let v = json!({"hooks": {
            "PreToolUse": [{"matcher": "Bash|Edit", "hooks": [{"type": "command", "command": "echo pre", "timeout": 9000}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "echo stop"}, {"type": "peck", "ask": "x", "run": "git_commit"}]}],
            "Nope": [{"hooks": [{"type": "command", "command": "x"}]}]
        }});
        let hooks = parse(&v, ".claude/settings.json", None);
        assert_eq!(hooks.len(), 2, "只认 command，不认识的时刻跳过");
        assert_eq!(hooks[0].timeout_secs, MAX_TIMEOUT);
        let pre = &hooks[0];
        assert!(pre.matches(Some("bash_run")) && pre.matches(Some("write_patch")) && pre.matches(Some("Edit")));
        assert!(!pre.matches(Some("read_file")), "整个名字对上才算");
        assert!(hook(Event::PreToolUse, Some("mcp__.*"), "x").matches(Some("mcp__dw__list")));
        assert!(hook(Event::PreToolUse, None, "x").matches(Some("anything")));
        assert_ne!(hook(Event::Stop, None, "a").rule(), hook(Event::Stop, None, "b").rule(), "命令改了指纹就变");
        let set = HookSet { hooks };
        assert_eq!(set.for_event(Event::PreToolUse, Some("read_file")).len(), 0);
        assert_eq!(set.for_event(Event::Stop, None).len(), 1);
    }

    #[test]
    fn exit_codes_and_json_follow_claude_code() {
        let ran = |code: i32, out: &str, err: &str| Ran { code: Some(code), stdout: out.into(), stderr: err.into(), ..Default::default() };
        let v = verdict(Event::PreToolUse, &ran(2, "", "不许改 .env"));
        assert_eq!(v.block.as_deref(), Some("不许改 .env"));
        assert_eq!(verdict(Event::PreCompact, &ran(2, "", "x")).block, None, "压缩前拦不住");
        let v = verdict(Event::PostToolUse, &ran(0, r#"{"decision":"block","reason":"测试没过"}"#, ""));
        assert_eq!(v.block.as_deref(), Some("测试没过"));
        let v = verdict(Event::PreToolUse, &ran(0, r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"只读目录"}}"#, ""));
        assert_eq!(v.block.as_deref(), Some("只读目录"));
        let v = verdict(Event::UserPromptSubmit, &ran(0, "今天是周一", ""));
        assert_eq!((v.block, v.context.as_deref()), (None, Some("今天是周一")));
        assert_eq!(verdict(Event::PostToolUse, &ran(0, "格式化好了", "")).context, None, "别的时刻 stdout 只给人看");
        let v = verdict(Event::Stop, &ran(1, "", "lint 挂了"));
        assert_eq!((v.block, v.error.as_deref()), (None, Some("退出码 1：lint 挂了")));
        let input = input(Event::PreToolUse, "s1", Path::new("/p"), PermissionLevel::Edit, json!({"tool_name": "bash_run", "tool_input": {"command": "ls"}}));
        assert_eq!((input["tool_name"].as_str(), input["peck_tool_name"].as_str(), input["permission_mode"].as_str()), (Some("Bash"), Some("bash_run"), Some("default")));
    }

    #[tokio::test]
    async fn commands_get_json_on_stdin_and_time_out() {
        let dir = std::env::temp_dir();
        let env = vec![("PATH".to_string(), std::env::var("PATH").unwrap_or_default())];
        let echo = hook(Event::PreToolUse, None, r#"cat | grep -q '"tool_name":"Bash"' && echo seen; echo "$PECK_HOOK_EVENT" >&2; exit 2"#);
        let ran = run(&echo, &json!({"tool_name": "Bash"}), &dir, &env).await;
        assert_eq!((ran.code, ran.stdout.trim(), ran.stderr.trim()), (Some(2), "seen", "PreToolUse"));
        let mut slow = hook(Event::Stop, None, "sleep 5");
        slow.timeout_secs = 1;
        let started = std::time::Instant::now();
        let ran = run(&slow, &json!({}), &dir, &env).await;
        assert!(ran.timed_out && started.elapsed() < Duration::from_secs(4));
    }
}
