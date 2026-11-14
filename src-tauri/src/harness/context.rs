//! Peck 循环的上下文（v0.6 Phase 7）：模型每次看到什么、多大。
//!
//! 分层（前面的不常变，打缓存断点；后面的每次都变）：
//! 1. Peck 自己的说明 + 工具定义（系统提示第一块，缓存）；
//! 2. 项目说明（`AGENTS.md`，没有就读 `CLAUDE.md`）+ 站主批过的记忆（系统提示第二块，缓存）；
//! 3. 对话（Anthropic 在最后一条消息上再打一个断点，见 `convert/anthropic.rs`）；
//! 4. 每次发一句话时带上的环境（`<peck-context>`）：系统、git、任务清单、这个任务改过的文件、
//!    验证命令、在跑的后台命令；放在这一句话的前面，不进系统提示，免得每次把缓存打穿。
//!
//! 大小用一个粗估（中日韩字一个算一个 token，其他四个字符一个），有实报的用量时按实报校准（`scale`）。

use crate::gateway::event::{ContextPart, PermissionLevel, PlanStep, PlanStatus};
use crate::gateway::{Message, Part, ToolSpec};
use std::path::Path;
use std::process::{Command, Stdio};

/// 项目说明按这个顺序找，用第一个有的。
pub const INSTRUCTION_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
/// 项目说明最多带这么多（再长的截掉，写明）。
pub const INSTRUCTIONS_MAX: usize = 32 * 1024;
/// 每次带上的环境块的开头（压缩时据此认出旧的环境块）。
pub const CONTEXT_TAG: &str = "<peck-context>";
/// 引用的旧任务（`@任务`）的开头。
pub const REF_TAG: &str = "<referenced-task";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instructions {
    /// `AGENTS.md` 或 `CLAUDE.md`。
    pub file: String,
    pub text: String,
    pub truncated: bool,
}

/// 系统提示的第二层：项目说明 + 记忆。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectLayer {
    pub instructions: Option<Instructions>,
    /// 站主批过的记忆（旧的在前）。
    pub memories: Vec<String>,
}

/// 读项目根下的说明文件：`AGENTS.md` 优先，没有就 `CLAUDE.md`。空文件当没有。
pub fn read_instructions(root: &Path) -> Option<Instructions> {
    for name in INSTRUCTION_FILES {
        let path = root.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        // 符号链接只认指到项目里的（不让它把项目外的文件读进模型上下文）。
        let real = if meta.file_type().is_symlink() {
            match path.canonicalize() {
                Ok(p) if p.starts_with(root) => p,
                _ => continue,
            }
        } else {
            path
        };
        let Ok(bytes) = std::fs::read(&real) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        if text.trim().is_empty() {
            continue;
        }
        let truncated = text.len() > INSTRUCTIONS_MAX;
        let kept = super::tools::clip_bytes(&text, INSTRUCTIONS_MAX).trim_end().to_string();
        return Some(Instructions { file: name.to_string(), text: kept, truncated });
    }
    None
}

impl ProjectLayer {
    pub fn load(root: &Path, memories: Vec<String>) -> Self {
        Self { instructions: read_instructions(root), memories }
    }

    /// 系统提示的第二块；什么都没有为空。
    pub fn text(&self) -> Option<String> {
        let mut out = String::new();
        if let Some(i) = &self.instructions {
            out.push_str(&format!(
                "# Project instructions ({})\nThe project's own instructions for coding agents. Follow them unless the user says otherwise.\n\n{}\n",
                i.file, i.text
            ));
            if i.truncated {
                out.push_str(&format!("\n[{} is longer than {} KB; the rest was cut. Read the file if you need it.]\n", i.file, INSTRUCTIONS_MAX / 1024));
            }
        }
        if !self.memories.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("# Memory\nNotes about this project that the user approved for you to remember:\n");
            for m in &self.memories {
                out.push_str(&format!("- {}\n", m.trim()));
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// 每次发一句话时带上的环境要的东西。
#[derive(Debug, Clone, Default)]
pub struct EnvInput {
    pub level: Option<PermissionLevel>,
    /// 最近一份任务清单。
    pub plan: Vec<PlanStep>,
    /// 这个任务（之前几次 + 这一次）改过的文件。
    pub changed: Vec<String>,
    /// 验证命令：名字 + 命令。
    pub checks: Vec<(String, String)>,
    /// 在跑的后台命令（一行一个，给模型看）。
    pub background: Vec<String>,
    /// 目标模式：要过的那几条。
    pub goal: Vec<String>,
    /// 项目的其他文件夹（v0.7）：路径、能不能改。
    pub folders: Vec<(String, bool)>,
    /// 项目自己的环境变量的名字（值不写进上下文）。
    pub env_names: Vec<String>,
    /// 在独立工作区里做：主文件夹（v0.7）。
    pub worktree_of: Option<String>,
}

/// 环境块：系统、git、任务清单、改过的文件、验证命令、后台命令。给模型看，写英文。
pub fn environment_block(root: &Path, input: &EnvInput) -> String {
    let mut lines = vec![CONTEXT_TAG.to_string()];
    lines.push(format!(
        "Environment: {} {} · project root {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        root.display()
    ));
    if let Some(main) = &input.worktree_of {
        lines.push(format!(
            "You are working in a separate git worktree of the project (the main folder is {main}). Work only here; the user applies your changes back to the main folder after reviewing them."
        ));
    }
    if !input.folders.is_empty() {
        let list: Vec<String> = input
            .folders
            .iter()
            .map(|(p, w)| format!("{p} ({})", if *w { "writable" } else { "read-only" }))
            .collect();
        lines.push(format!(
            "Other folders in this project (use absolute paths with the file tools; read-only ones cannot be written): {}",
            list.join(" · ")
        ));
    }
    if !input.env_names.is_empty() {
        lines.push(format!("Project environment variables set for commands: {}", input.env_names.join(", ")));
    }
    if let Some(git) = git_summary(root) {
        lines.push(git);
    } else {
        lines.push("Git: not a git repository.".into());
    }
    if !input.plan.is_empty() {
        lines.push("Task checklist (latest):".into());
        for s in &input.plan {
            let mark = match s.status {
                PlanStatus::Completed => "[x]",
                PlanStatus::InProgress => "[>]",
                PlanStatus::Pending => "[ ]",
            };
            lines.push(format!("{mark} {}", s.text));
        }
    }
    if !input.changed.is_empty() {
        let shown: Vec<&str> = input.changed.iter().take(40).map(String::as_str).collect();
        let more = input.changed.len().saturating_sub(shown.len());
        lines.push(format!(
            "Files changed in this task so far ({}): {}{}",
            input.changed.len(),
            shown.join(", "),
            if more > 0 { format!(" … and {more} more") } else { String::new() }
        ));
    }
    if !input.checks.is_empty() {
        let list: Vec<String> = input.checks.iter().map(|(n, c)| format!("{n}: `{c}`")).collect();
        lines.push(format!(
            "Verify commands (Peck runs them for you after you edit and when you stop; you get the output if they fail): {}",
            list.join(" · ")
        ));
    }
    if !input.goal.is_empty() {
        lines.push(format!(
            "Goal mode: keep working until these pass: {}. Peck runs them each time you stop.",
            input.goal.join(", ")
        ));
    }
    if !input.background.is_empty() {
        lines.push("Background commands:".into());
        lines.extend(input.background.iter().map(|b| format!("- {b}")));
    }
    lines.push("</peck-context>".into());
    lines.join("\n")
}

fn git_bin() -> &'static str {
    if Path::new("/usr/bin/git").exists() {
        "/usr/bin/git"
    } else {
        "git"
    }
}

/// 分支、没提交的改动（最多 20 条）、最近 5 个提交。不碰用户仓库的 index（`GIT_OPTIONAL_LOCKS=0`）。
pub fn git_summary(root: &Path) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new(git_bin())
            .current_dir(root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_PAGER", "cat")
            .env("LC_ALL", "C")
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let status = git(&["status", "--porcelain=v1", "-b", "--untracked-files=normal"])?;
    let mut lines = status.lines();
    let branch = lines
        .next()
        .and_then(|l| l.strip_prefix("## "))
        .unwrap_or("(unknown)")
        .to_string();
    let changes: Vec<&str> = lines.filter(|l| !l.trim().is_empty()).collect();
    let mut out = vec![format!("Git branch: {branch}")];
    if changes.is_empty() {
        out.push("Uncommitted changes: none".into());
    } else {
        let shown: Vec<&str> = changes.iter().take(20).map(|l| l.trim()).collect();
        let more = changes.len().saturating_sub(shown.len());
        out.push(format!(
            "Uncommitted changes ({}): {}{}",
            changes.len(),
            shown.join(" · "),
            if more > 0 { format!(" … and {more} more") } else { String::new() }
        ));
    }
    if let Some(log) = git(&["log", "-5", "--oneline", "--no-decorate", "--no-color"]) {
        let commits: Vec<String> = log.lines().map(|l| format!("- {}", super::tools::clip_bytes(l, 120))).collect();
        if !commits.is_empty() {
            out.push(format!("Recent commits:\n{}", commits.join("\n")));
        }
    }
    Some(out.join("\n"))
}

// ───────────────────────── 大小 ─────────────────────────

/// 粗估 tokens：中日韩这类宽字一个算一个，其他四个字符算一个。
pub fn estimate(text: &str) -> u64 {
    let (mut wide, mut narrow) = (0u64, 0u64);
    for c in text.chars() {
        if (c as u32) >= 0x2E80 {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    wide + narrow.div_ceil(4)
}

/// 上下文按块估的大小（还没校准）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Breakdown {
    pub system: u64,
    pub project: u64,
    pub conversation: u64,
    pub tools: u64,
}

impl Breakdown {
    pub fn total(&self) -> u64 {
        self.system + self.project + self.conversation + self.tools
    }

    /// 乘上校准系数，给界面的几块。
    pub fn parts(&self, scale: f64) -> Vec<ContextPart> {
        [("system", self.system), ("project", self.project), ("conversation", self.conversation), ("tools", self.tools)]
            .into_iter()
            .map(|(key, n)| ContextPart { key: key.into(), tokens: scaled(n, scale) })
            .collect()
    }
}

pub fn scaled(n: u64, scale: f64) -> u64 {
    (n as f64 * scale).round().max(0.0) as u64
}

/// 系统提示（Peck 的说明 + 工具定义）、项目层、对话、工具输出各多大。
pub fn breakdown(base: &str, tools: &[ToolSpec], project: Option<&str>, messages: &[Message]) -> Breakdown {
    let tool_text: u64 = tools
        .iter()
        .map(|t| estimate(&t.name) + estimate(&t.description) + estimate(&t.input_schema.to_string()))
        .sum();
    let (conversation, tool_out) = messages_estimate(messages);
    Breakdown {
        system: estimate(base) + tool_text,
        project: project.map(estimate).unwrap_or(0),
        conversation,
        tools: tool_out,
    }
}

/// （对话和思考、工具输出）。每条消息另加 4 个 token 的格式开销。
pub fn messages_estimate(messages: &[Message]) -> (u64, u64) {
    let (mut conv, mut tools) = (0u64, 0u64);
    for m in messages {
        conv += 4;
        for p in &m.parts {
            match p {
                Part::Text { text } => conv += estimate(text),
                Part::Reasoning { text, .. } => conv += estimate(text),
                Part::ToolCall { name, input, .. } => conv += estimate(name) + estimate(&input.to_string()) + 4,
                Part::ToolResult { output, .. } => tools += estimate(output) + 4,
            }
        }
    }
    (conv, tools)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::tools::testing::TempProject;
    use crate::gateway::Role;

    #[test]
    fn agents_md_wins_then_claude_md_and_long_ones_are_cut() {
        let p = TempProject::new();
        assert_eq!(read_instructions(&p.dir), None);
        p.write("CLAUDE.md", "用中文写文档。\n");
        assert_eq!(read_instructions(&p.dir).unwrap().file, "CLAUDE.md");
        p.write("AGENTS.md", "   \n");
        assert_eq!(read_instructions(&p.dir).unwrap().file, "CLAUDE.md", "空的 AGENTS.md 当没有");
        p.write("AGENTS.md", "见 CLAUDE.md\n");
        let i = read_instructions(&p.dir).unwrap();
        assert_eq!((i.file.as_str(), i.text.as_str(), i.truncated), ("AGENTS.md", "见 CLAUDE.md", false));
        p.write("AGENTS.md", &"x".repeat(INSTRUCTIONS_MAX + 10));
        let i = read_instructions(&p.dir).unwrap();
        assert!(i.truncated && i.text.len() == INSTRUCTIONS_MAX);
        let layer = ProjectLayer { instructions: Some(i), memories: vec!["测试用 pnpm test".into()] };
        let text = layer.text().unwrap();
        assert!(text.contains("# Project instructions (AGENTS.md)") && text.contains("cut") && text.contains("- 测试用 pnpm test"));
        assert_eq!(ProjectLayer::default().text(), None);
    }

    #[cfg(unix)]
    #[test]
    fn instructions_symlinked_outside_are_ignored() {
        let p = TempProject::new();
        let outside = std::env::temp_dir().join(format!("peck-out-{}.md", uuid::Uuid::new_v4()));
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, p.dir.join("AGENTS.md")).unwrap();
        p.write("CLAUDE.md", "inside");
        assert_eq!(read_instructions(&p.dir).unwrap().text, "inside");
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn environment_block_has_git_plan_changes_and_checks() {
        let p = TempProject::new();
        p.write("a.txt", "1\n");
        let git = |args: &[&str]| {
            Command::new(git_bin()).current_dir(&p.dir).args(args).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@e").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@e").output().unwrap()
        };
        let block = environment_block(&p.dir, &EnvInput::default());
        assert!(block.starts_with(CONTEXT_TAG) && block.contains("not a git repository"));
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "first commit"]);
        p.write("b.txt", "2\n");
        let index_before = std::fs::metadata(p.dir.join(".git/index")).unwrap().modified().unwrap();
        let input = EnvInput {
            level: Some(PermissionLevel::Edit),
            plan: vec![PlanStep { text: "改标题".into(), status: PlanStatus::InProgress }],
            changed: vec!["a.txt".into()],
            checks: vec![("check".into(), "cargo check".into())],
            background: vec!["dev server `pnpm dev` running".into()],
            goal: vec![],
            folders: vec![("/other/lib".into(), false)],
            env_names: vec!["API_BASE".into()],
            worktree_of: None,
        };
        let block = environment_block(&p.dir, &input);
        assert!(block.contains("Git branch: main"), "{block}");
        assert!(block.contains("?? b.txt") && block.contains("first commit"), "{block}");
        assert!(block.contains("[>] 改标题") && block.contains("a.txt") && block.contains("check: `cargo check`") && block.contains("pnpm dev"));
        assert!(block.ends_with("</peck-context>"));
        assert!(block.contains("/other/lib (read-only)") && block.contains("API_BASE"));
        let index_after = std::fs::metadata(p.dir.join(".git/index")).unwrap().modified().unwrap();
        assert_eq!(index_before, index_after, "不动用户仓库的 index");
    }

    #[test]
    fn estimates_count_cjk_as_one_and_split_by_part() {
        assert_eq!(estimate("abcd"), 1);
        assert_eq!(estimate("改标题"), 3);
        assert_eq!(estimate(""), 0);
        let messages = vec![
            Message { role: Role::User, parts: vec![Part::Text { text: "a".repeat(400) }] },
            Message { role: Role::User, parts: vec![Part::ToolResult { call_id: "c".into(), output: "b".repeat(4000), is_error: false }] },
        ];
        let b = breakdown("x".repeat(40).as_str(), &[], Some("项目"), &messages);
        assert_eq!((b.system, b.project), (10, 2));
        assert!(b.tools > b.conversation);
        let parts = b.parts(2.0);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0].tokens, 20);
    }
}
