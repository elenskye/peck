//! Peck 循环的十二个工具（`update_plan`、`ask_user`、`memory`、`search_tasks`、`bash_background` 由循环自己处理：
//! 要等站主、要发事件、要读任务记录或者要起后台进程）。v0.6 加了后三个。
//!
//! 读的工具只看项目里（外加这个任务落盘的命令输出）；改的工具只改项目里、不碰 `.git`；
//! 跑命令在沙箱里：默认联网，只能写项目、临时目录和 Peck 给的 HOME（[`bash`]）。

pub mod bash;
pub mod fs;
pub mod patch;
pub mod search;

use crate::gateway::event::{FileChange, FileDiff, PermissionLevel, ToolKind};
use crate::gateway::ToolSpec;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

pub const LIST_DIRECTORY: &str = "list_directory";
pub const READ_FILE: &str = "read_file";
pub const SEARCH_TEXT: &str = "search_text";
pub const FIND_FILES: &str = "find_files";
pub const WRITE_FILE: &str = "write_file";
pub const WRITE_PATCH: &str = "write_patch";
pub const BASH_RUN: &str = "bash_run";
pub const UPDATE_PLAN: &str = "update_plan";
pub const ASK_USER: &str = "ask_user";
pub const MEMORY: &str = "memory";
pub const SEARCH_TASKS: &str = "search_tasks";
pub const BASH_BACKGROUND: &str = "bash_background";

pub const ALL: [&str; 12] = [
    LIST_DIRECTORY,
    READ_FILE,
    SEARCH_TEXT,
    FIND_FILES,
    WRITE_FILE,
    WRITE_PATCH,
    BASH_RUN,
    BASH_BACKGROUND,
    UPDATE_PLAN,
    ASK_USER,
    MEMORY,
    SEARCH_TASKS,
];

/// 一个文件读到时的样子：覆盖前要它没变过。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub hash: u64,
    pub len: u64,
}

pub fn stamp_of(bytes: &[u8]) -> Stamp {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Stamp {
        hash: h.finish(),
        len: bytes.len() as u64,
    }
}

/// 一次任务里工具共用的东西。
pub struct ToolContext {
    /// 项目根（规范化过的绝对路径）。
    pub root: PathBuf,
    pub level: PermissionLevel,
    /// 命令输出太长时整份落盘的目录（这个任务一个）。
    pub spill_dir: PathBuf,
    /// 跑命令用的环境（站主 shell 的那份，去掉像 Key 的变量）。
    pub env: Vec<(String, String)>,
    /// 命令的 HOME 和临时目录放哪（Peck 数据目录下，每个项目一个）。
    pub runtime_dir: PathBuf,
    /// 这个任务里读过的文件：绝对路径 → 读到时的样子。
    pub reads: Mutex<HashMap<PathBuf, Stamp>>,
    /// 这个任务里改过的文件第一次改之前的内容（`None` 是原来没有），给回合的 Diff 汇总用。
    pub originals: Mutex<Vec<(String, Option<String>)>>,
    /// 落盘文件的编号。
    pub spill_count: std::sync::atomic::AtomicU32,
}

impl ToolContext {
    pub fn new(root: &Path, level: PermissionLevel, spill_dir: PathBuf, runtime_dir: PathBuf, env: Vec<(String, String)>) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|_| "项目文件夹打不开。".to_string())?;
        Ok(Self {
            root,
            level,
            spill_dir,
            env,
            runtime_dir,
            reads: Mutex::new(HashMap::new()),
            originals: Mutex::new(Vec::new()),
            spill_count: Default::default(),
        })
    }

    /// 相对项目根的显示路径（`/` 分隔）；不在项目里就原样。
    pub fn rel(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(r) if r.as_os_str().is_empty() => ".".into(),
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    pub fn remember_read(&self, path: &Path, bytes: &[u8]) {
        if let Ok(mut reads) = self.reads.lock() {
            reads.insert(path.to_path_buf(), stamp_of(bytes));
        }
    }

    pub fn read_stamp(&self, path: &Path) -> Option<Stamp> {
        self.reads.lock().ok().and_then(|r| r.get(path).copied())
    }

    /// 记下第一次改之前的内容。
    pub fn remember_original(&self, rel: &str, before: Option<&str>) {
        if let Ok(mut o) = self.originals.lock() {
            if !o.iter().any(|(p, _)| p == rel) {
                o.push((rel.to_string(), before.map(str::to_string)));
            }
        }
    }

    /// 这个任务到现在为止改了哪些文件（和第一次改之前比）。
    pub fn diff_summary(&self) -> Vec<FileDiff> {
        let originals = self.originals.lock().map(|o| o.clone()).unwrap_or_default();
        originals
            .into_iter()
            .filter_map(|(rel, before)| {
                let now = std::fs::read_to_string(self.root.join(&rel)).ok();
                if now == before {
                    return None;
                }
                Some(file_diff(&rel, before.as_deref(), now.as_deref()))
            })
            .collect()
    }

    /// 这个文件是不是这个任务落盘的命令输出（`read_file` 能读）。
    pub fn is_spill(&self, path: &Path) -> bool {
        path.starts_with(&self.spill_dir)
    }
}

/// 一个工具跑完的结果。
#[derive(Debug, Clone, Default)]
pub struct ToolResult {
    pub ok: bool,
    /// 给模型的文字。
    pub text: String,
    pub exit_code: Option<i32>,
    pub diff: Vec<FileDiff>,
}

impl ToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            ..Default::default()
        }
    }
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
            ..Default::default()
        }
    }
}

/// 两份内容的 Diff（统一格式，前后 3 行）。
pub fn file_diff(rel: &str, before: Option<&str>, after: Option<&str>) -> FileDiff {
    let old = before.unwrap_or("");
    let new = after.unwrap_or("");
    let diff = similar::TextDiff::from_lines(old, new);
    let (mut additions, mut deletions) = (0u32, 0u32);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => additions += 1,
            similar::ChangeTag::Delete => deletions += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    let patch = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{rel}"), &format!("b/{rel}"))
        .to_string();
    FileDiff {
        path: rel.to_string(),
        change: match (before, after) {
            (None, Some(_)) => FileChange::Added,
            (Some(_), None) => FileChange::Deleted,
            _ => FileChange::Modified,
        },
        old_path: None,
        additions: Some(additions),
        deletions: Some(deletions),
        patch: Some(patch),
    }
}

/// 模型给的路径 → 项目里的绝对路径。可以是相对路径，也可以是项目里的绝对路径；
/// 不能出项目（`..`、指到外面的符号链接）、不能碰 `.git`。
pub fn resolve(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let given = Path::new(if raw.is_empty() { "." } else { raw });
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    if joined.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("路径不能带 ..：{raw}"));
    }
    let inside = joined.strip_prefix(root).map_err(|_| format!("只能用项目里的文件：{raw}"))?;
    if touches_git(inside) {
        return Err("不能碰 .git。".into());
    }
    // 找最近的、存在的上一级，规范化以后还得在项目里（符号链接不能指到外面）。
    let mut ancestor = joined.as_path();
    while std::fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor.parent().ok_or_else(|| format!("路径不对：{raw}"))?;
    }
    let canonical = ancestor
        .canonicalize()
        .map_err(|_| format!("路径指向一个不存在的地方：{raw}"))?;
    if !canonical.starts_with(root) {
        return Err(format!("这个路径经符号链接指到了项目外面：{raw}"));
    }
    let rest = joined.strip_prefix(ancestor).unwrap_or(Path::new(""));
    let resolved = if rest.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(rest)
    };
    // 规范化以后再看一次：指向 .git 的符号链接、大小写不同的 `.GIT`（APFS 不分大小写）都挡掉。
    // 文件工具在沙箱外跑，写进 .git（hooks、config）等于让下一次 git 命令跑任意代码。
    if resolved.strip_prefix(root).map(touches_git).unwrap_or(true) {
        return Err("不能碰 .git。".into());
    }
    Ok(resolved)
}

/// 路径里有没有 `.git` 这一级（不分大小写）。
fn touches_git(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str().to_str().is_some_and(|s| s.eq_ignore_ascii_case(".git")))
}

/// 按字节截断，不切坏多字节字符。
pub fn clip_bytes(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// 从结尾往前取最多 `limit` 字节。
pub fn tail_bytes(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

pub fn kind_of(name: &str) -> ToolKind {
    match name {
        LIST_DIRECTORY | READ_FILE => ToolKind::Read,
        SEARCH_TEXT | FIND_FILES | SEARCH_TASKS => ToolKind::Search,
        WRITE_FILE | WRITE_PATCH => ToolKind::Edit,
        BASH_RUN | BASH_BACKGROUND => ToolKind::Run,
        _ => ToolKind::Other,
    }
}

/// 给人看的一行：「读 src/App.tsx」「跑 npm test」。
pub fn title_of(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let short = |t: String| {
        let one = t.lines().next().unwrap_or("").to_string();
        if one.chars().count() > 80 {
            format!("{}…", one.chars().take(80).collect::<String>())
        } else if t.lines().count() > 1 {
            format!("{one} …")
        } else {
            one
        }
    };
    match name {
        LIST_DIRECTORY => {
            let p = s("path");
            format!("看 {}", if p.is_empty() || p == "." { "项目根目录".into() } else { p })
        }
        READ_FILE => format!("读 {}", s("path")),
        SEARCH_TEXT => format!("搜「{}」", short(s("pattern"))),
        FIND_FILES => format!("找 {}", s("pattern")),
        WRITE_FILE => format!("写 {}", s("path")),
        WRITE_PATCH => {
            let edits = input.get("edits").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut paths: Vec<String> = Vec::new();
            for e in &edits {
                if let Some(p) = e.get("path").and_then(Value::as_str) {
                    if !paths.iter().any(|x| x == p) {
                        paths.push(p.to_string());
                    }
                }
            }
            match paths.len() {
                0 => "改文件".into(),
                1 if edits.len() > 1 => format!("改 {}（{} 处）", paths[0], edits.len()),
                1 => format!("改 {}", paths[0]),
                n => format!("改 {} 等 {n} 个文件", paths[0]),
            }
        }
        BASH_RUN => format!("跑 {}", short(s("command"))),
        BASH_BACKGROUND => match s("action").as_str() {
            "start" => format!("后台跑 {}", short(s("command"))),
            "stop" => format!("停后台命令 {}", s("id")),
            "read" => format!("看后台命令 {}", s("id")),
            _ => "看后台命令".into(),
        },
        UPDATE_PLAN => "更新任务清单".into(),
        ASK_USER => "问你".into(),
        MEMORY => "想记住几条".into(),
        SEARCH_TASKS => match (s("query"), input.get("number").and_then(Value::as_u64)) {
            (_, Some(n)) => format!("看任务 {n}"),
            (q, None) if !q.is_empty() => format!("搜旧任务「{}」", short(q)),
            _ => "看最近的任务".into(),
        },
        other => other.to_string(),
    }
}

/// 这一档能用哪些工具。ONE SHOT 只读、不弹卡片，去掉清单、提问、记忆和后台命令。
pub fn names_for(level: PermissionLevel, oneshot: bool) -> Vec<&'static str> {
    let read_only = [LIST_DIRECTORY, READ_FILE, SEARCH_TEXT, FIND_FILES, BASH_RUN, SEARCH_TASKS];
    if oneshot {
        return read_only.to_vec();
    }
    match level {
        PermissionLevel::Plan => {
            let mut v = read_only.to_vec();
            v.extend([BASH_BACKGROUND, UPDATE_PLAN, ASK_USER, MEMORY]);
            v
        }
        PermissionLevel::Edit | PermissionLevel::Auto => ALL.to_vec(),
    }
}

pub fn specs_for(names: &[&str]) -> Vec<ToolSpec> {
    names.iter().filter_map(|n| spec(n)).collect()
}

fn spec(name: &str) -> Option<ToolSpec> {
    let (description, schema) = match name {
        LIST_DIRECTORY => (
            "List files and folders in the project (respects .gitignore). Up to 300 entries.",
            json!({"type":"object","properties":{
                "path":{"type":"string","description":"Folder relative to the project root. Default: the root."},
                "depth":{"type":"integer","minimum":1,"maximum":3,"description":"How many levels to show (1-3). Default 1."}
            }}),
        ),
        READ_FILE => (
            "Read a text file with line numbers (6-wide, then a tab). Up to 2000 lines / 50KB per call; when truncated the result says where to continue. Also reads the full output files that bash_run saved.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "start_line":{"type":"integer","minimum":1,"description":"1-based. Default 1."},
                "line_count":{"type":"integer","minimum":1,"maximum":2000,"description":"Default 2000."}
            },"required":["path"]}),
        ),
        SEARCH_TEXT => (
            "Search file contents in the project like ripgrep (respects .gitignore). Literal by default; set regex=true for a regular expression.",
            json!({"type":"object","properties":{
                "pattern":{"type":"string"},
                "path":{"type":"string","description":"File or folder to search. Default: the project root."},
                "glob":{"type":"string","description":"Only files matching this glob, e.g. *.ts"},
                "regex":{"type":"boolean","description":"Default false."},
                "mode":{"type":"string","enum":["content","files","count"],"description":"content: matching lines; files: file names; count: matches per file. Default content."},
                "context":{"type":"integer","minimum":0,"maximum":10,"description":"Lines of context around each match (content mode)."},
                "max":{"type":"integer","minimum":1,"maximum":500,"description":"Max results. Default 100."},
                "offset":{"type":"integer","minimum":0,"description":"Skip this many results (paging)."}
            },"required":["pattern"]}),
        ),
        FIND_FILES => (
            "Find files by glob (e.g. **/*.rs, src/**/test_*.py), newest first. Up to 100.",
            json!({"type":"object","properties":{
                "pattern":{"type":"string"},
                "path":{"type":"string","description":"Folder to search in. Default: the project root."}
            },"required":["pattern"]}),
        ),
        WRITE_FILE => (
            "Create a file or replace a whole file. To overwrite an existing file you must have read it in this task and it must not have changed since. Prefer write_patch for edits.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "content":{"type":"string"}
            },"required":["path","content"]}),
        ),
        WRITE_PATCH => (
            "Edit existing files by replacing exact text: each edit finds old_text and replaces it with new_text. The whole batch is checked first and applied atomically: if any edit fails, nothing is written. old_text must match exactly one place unless replace_all is true. Include enough surrounding lines to be unique.",
            json!({"type":"object","properties":{
                "edits":{"type":"array","minItems":1,"maxItems":50,"items":{"type":"object","properties":{
                    "path":{"type":"string"},
                    "old_text":{"type":"string"},
                    "new_text":{"type":"string"},
                    "replace_all":{"type":"boolean"}
                },"required":["path","old_text","new_text"]}}
            },"required":["edits"]}),
        ),
        BASH_RUN => (
            "Run a shell command in the project. Network is on. It can only write inside the project, the temp dir and its own HOME; .git is read-only. stdin is empty; CI=1, PAGER=cat, NO_COLOR=1. Output over 16KB is saved to a file and you get the head, the tail and the file path (read it with read_file). Long-running servers are stopped when the command finishes or times out.",
            json!({"type":"object","properties":{
                "command":{"type":"string"},
                "cwd":{"type":"string","description":"Relative to the project root. Default: the root."},
                "timeout_sec":{"type":"integer","minimum":1,"maximum":600,"description":"Default 120."}
            },"required":["command"]}),
        ),
        BASH_BACKGROUND => (
            "Run a long-running command in the background (dev server, watcher, long test suite) without blocking, then check on it. action=start (command, cwd?, name?) returns an id and the first output; action=read (id, wait_sec? up to 60) returns the new output since your last read and whether it is still running; action=stop (id); action=list. Same sandbox and approval rules as bash_run, no timeout. Your background commands are stopped when the task ends. The project's dev servers (from .peck/verify.toml) are listed too: you can read them but not stop them.",
            json!({"type":"object","properties":{
                "action":{"type":"string","enum":["start","read","stop","list"]},
                "command":{"type":"string","description":"start: the shell command."},
                "cwd":{"type":"string","description":"start: relative to the project root. Default: the root."},
                "name":{"type":"string","description":"start: a short label."},
                "id":{"type":"string","description":"read / stop: the id from start or list."},
                "wait_sec":{"type":"integer","minimum":0,"maximum":60,"description":"read: wait up to this long for new output or exit. Default 0."}
            },"required":["action"]}),
        ),
        MEMORY => (
            "Propose notes worth remembering for future tasks in this project: conventions, commands, pitfalls, the user's stated preferences. They are saved only if the user approves them when the task ends. Keep each note one short sentence; don't propose trivia or things already in the project instructions.",
            json!({"type":"object","properties":{
                "notes":{"type":"array","minItems":1,"maxItems":5,"items":{"type":"string"}}
            },"required":["notes"]}),
        ),
        SEARCH_TASKS => (
            "Look up earlier tasks in this project: what was asked, how it ended, which files changed. query: words that must all appear (in the title, request, result or file paths); number: the full record of one task. Neither: the 10 most recent.",
            json!({"type":"object","properties":{
                "query":{"type":"string"},
                "number":{"type":"integer","minimum":1}
            }}),
        ),
        UPDATE_PLAN => (
            "Replace the task checklist shown to the user. At most one step may be in_progress.",
            json!({"type":"object","properties":{
                "steps":{"type":"array","items":{"type":"object","properties":{
                    "text":{"type":"string"},
                    "status":{"type":"string","enum":["pending","in_progress","completed"]}
                },"required":["text","status"]}},
                "explanation":{"type":"string"}
            },"required":["steps"]}),
        ),
        ASK_USER => (
            "Ask the user a question and wait for the answer. Use only when you cannot proceed without their decision.",
            json!({"type":"object","properties":{
                "question":{"type":"string"},
                "options":{"type":"array","items":{"type":"string"},"description":"Suggested answers. The user can also write their own."}
            },"required":["question"]}),
        ),
        _ => return None,
    };
    Some(ToolSpec {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: schema,
    })
}

/// 跑一个不用等站主的工具（`bash_run` 另走 [`bash::run`]，因为要能打断、流式出字）。
pub fn run_sync(ctx: &ToolContext, name: &str, input: &Value) -> ToolResult {
    let result = match name {
        LIST_DIRECTORY => fs::list_directory(ctx, input),
        READ_FILE => fs::read_file(ctx, input),
        FIND_FILES => fs::find_files(ctx, input),
        SEARCH_TEXT => search::search_text(ctx, input),
        WRITE_FILE => fs::write_file(ctx, input),
        WRITE_PATCH => patch::write_patch(ctx, input),
        other => Err(format!("没有叫 {other} 的工具。")),
    };
    result.unwrap_or_else(ToolResult::err)
}

pub(crate) fn str_arg<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

pub(crate) fn u64_arg(input: &Value, key: &str) -> Option<u64> {
    input.get(key).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

pub(crate) fn bool_arg(input: &Value, key: &str) -> Option<bool> {
    input.get(key).and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub struct TempProject {
        pub dir: PathBuf,
    }

    impl TempProject {
        pub fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("peck-harness-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir: dir.canonicalize().unwrap() }
        }
        pub fn write(&self, rel: &str, text: &str) -> &Self {
            let p = self.dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
            self
        }
        pub fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.dir.join(rel)).unwrap()
        }
        pub fn ctx(&self, level: PermissionLevel) -> ToolContext {
            let spill = self.dir.with_extension("spill");
            let runtime = self.dir.with_extension("runtime");
            let env = vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())];
            ToolContext::new(&self.dir, level, spill, runtime, env).unwrap()
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
            let _ = std::fs::remove_dir_all(self.dir.with_extension("spill"));
            let _ = std::fs::remove_dir_all(self.dir.with_extension("runtime"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TempProject;
    use super::*;

    #[test]
    fn paths_stay_inside_the_project_and_away_from_git() {
        let p = TempProject::new();
        p.write("src/a.txt", "a");
        std::fs::create_dir_all(p.dir.join(".git")).unwrap();
        let root = &p.dir;
        assert_eq!(resolve(root, "src/a.txt").unwrap(), root.join("src/a.txt"));
        assert_eq!(resolve(root, "").unwrap(), *root);
        assert_eq!(resolve(root, "new/dir/b.txt").unwrap(), root.join("new/dir/b.txt"));
        assert_eq!(resolve(root, &root.join("src/a.txt").to_string_lossy()).unwrap(), root.join("src/a.txt"));
        assert!(resolve(root, "../x").is_err());
        assert!(resolve(root, "src/../../x").is_err());
        assert!(resolve(root, "/etc/passwd").is_err());
        assert!(resolve(root, ".git/config").is_err());
        assert!(resolve(root, ".GIT/config").is_err(), "APFS 不分大小写");
        assert!(resolve(root, "sub/.Git/hooks/pre-commit").is_err());
        assert!(resolve(root, ".gitignore").is_ok());
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir().join(format!("peck-out-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            assert!(resolve(root, "link/x.txt").unwrap_err().contains("符号链接"));
            let _ = std::fs::remove_dir_all(outside);
            // 项目里指向 .git 的符号链接（比如模型用 bash_run 建的）也不能当后门。
            std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
            std::os::unix::fs::symlink(root.join(".git"), root.join("g")).unwrap();
            assert!(resolve(root, "g/hooks/pre-commit").unwrap_err().contains(".git"));
            assert!(resolve(root, "g").unwrap_err().contains(".git"));
        }
    }

    #[test]
    fn tools_per_level() {
        assert!(!names_for(PermissionLevel::Plan, false).contains(&WRITE_FILE));
        assert!(!names_for(PermissionLevel::Plan, false).contains(&WRITE_PATCH));
        assert!(names_for(PermissionLevel::Plan, false).contains(&ASK_USER));
        assert!(names_for(PermissionLevel::Plan, false).contains(&MEMORY));
        assert_eq!(names_for(PermissionLevel::Edit, false).len(), 12);
        assert_eq!(names_for(PermissionLevel::Auto, false).len(), 12);
        let oneshot = names_for(PermissionLevel::Plan, true);
        assert!(!oneshot.contains(&ASK_USER) && !oneshot.contains(&WRITE_PATCH) && !oneshot.contains(&MEMORY) && !oneshot.contains(&BASH_BACKGROUND));
        assert!(oneshot.contains(&SEARCH_TASKS));
        assert_eq!(specs_for(&ALL).len(), 12);
    }

    #[test]
    fn titles_read_like_a_sentence() {
        assert_eq!(title_of(READ_FILE, &json!({"path":"src/App.tsx"})), "读 src/App.tsx");
        assert_eq!(title_of(BASH_RUN, &json!({"command":"npm test"})), "跑 npm test");
        assert_eq!(title_of(LIST_DIRECTORY, &json!({})), "看 项目根目录");
        assert_eq!(
            title_of(WRITE_PATCH, &json!({"edits":[{"path":"a"},{"path":"a"}]})),
            "改 a（2 处）"
        );
    }

    #[test]
    fn diffs_count_lines() {
        let d = file_diff("a.txt", Some("x\ny\n"), Some("x\nz\nw\n"));
        assert_eq!((d.additions, d.deletions), (Some(2), Some(1)));
        assert!(d.patch.unwrap().contains("+z"));
        assert_eq!(file_diff("n.txt", None, Some("1\n")).change, FileChange::Added);
    }
}
