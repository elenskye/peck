//! `git_commit`（v1.1.0，Git Plan 插件要的工具）：替模型、也替 Peck 自己的收尾钩子在项目的 git 仓库里提交。
//!
//! 只会提交：`git add` 指定的文件、`git commit --only`。碰不到 push、amend、rebase、reset，不改 git 配置，不署名。
//! - 说明必须是一行 `v<x.y.z>: <description>`，带 `Co-Authored-By`、「Generated with」这类署名的拒绝。
//! - 只提交点名的文件（没点名就是工作区里全部没提交的改动）；被 `.gitignore` 忽略的不会进去。站主先前 `git add` 过、
//!   这次没点名的别的文件不带上（`--only`）。
//! - 不是 git 项目、HEAD 是游离的（独立工作区里就是）、合并 / cherry-pick / revert 到一半（[`committable`]）不提交，
//!   而且在 `git add` 之前就拦下，不动站主的暂存区。仓库自己的 pre-commit 这类钩子照常跑（用站主 shell 的 PATH，
//!   不然从访达打开的 App 找不到 husky、git-lfs 这些）；一条 git 最多等 [`TIMEOUT`]，超了就结束它。
//! - 路径按字面算（`GIT_LITERAL_PATHSPECS`）：`app/[id]/page.tsx` 不当通配，`:` 开头的不当魔法路径。
//! - 用仓库配好的 `user.name` / `user.email`；没配 git 自己会报错，原样回给模型。
//!
//! 谁能用：项目里装着声明了它的插件（`project_plugins::tools_on`）。Peck Loop 直接调，CLI 经本机 MCP 调；
//! PLAN 不给，EDIT 问一次（「总是允许」记成 [`RULE`]），AUTO 不问。

use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 一条 git 命令最多等多久（提交时仓库自己的钩子也算在里面）。
const TIMEOUT: Duration = Duration::from_secs(180);

pub const GIT_COMMIT: &str = "git_commit";
/// EDIT 档「这个项目总是允许」的那条规则。
pub const RULE: &str = "git-commit";

pub const DESCRIPTION: &str = "Commit changes in this project's git repository. It can only commit: no push, no amend, no history rewriting, no signature lines. message: exactly one line `v<version>: <description>` (the version this round of work is for, then a short lowercase English summary, e.g. `v1.0.1: fix some problems`). paths: the files to commit (project-relative); omit to commit every uncommitted change in the working tree. Use this instead of running `git commit` yourself. Peck already commits each finished round on its own when the project has the Git Plan plugin, so only call this for an extra commit in the middle of a round or when the user asks.";

pub fn schema() -> Value {
    json!({"type":"object","properties":{
        "message":{"type":"string","description":"One line: v<x.y.z>: <short lowercase description>."},
        "paths":{"type":"array","items":{"type":"string"},"description":"Project-relative files or folders to commit. Omit for all uncommitted changes."}
    },"required":["message"]})
}

/// 一笔提交。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// 短哈希。
    pub hash: String,
    pub message: String,
    /// 这一笔里几个文件。
    pub files: usize,
}

/// 从模型的回答里取出提交说明：第一行不空的，去掉包着的反引号、引号和开头的列表记号。
pub fn first_line(answer: &str) -> String {
    let line = answer.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("```")).unwrap_or("");
    line.trim_start_matches(['-', '*', '>', ' ']).trim_matches(['`', '"', '\'', '「', '」', '“', '”', ' ']).to_string()
}

/// 校验提交说明：一行、`v<x.y.z>: <description>`、不带署名。合格的原样给回（去掉首尾空白）。
pub fn valid_message(message: &str) -> Result<String, String> {
    let msg = message.trim();
    let format = "提交说明要写成一行 `v<版本>: <description>`，比如 `v1.0.1: fix some problems`。";
    if msg.is_empty() {
        return Err(format!("没有提交说明。{format}"));
    }
    let lower = msg.to_lowercase();
    if ["co-authored-by", "generated with", "signed-off-by", "🤖"].iter().any(|s| lower.contains(s)) {
        return Err("提交说明里不能署名（Co-Authored-By、Generated with 这类）。去掉再提交。".into());
    }
    if msg.lines().count() > 1 {
        return Err(format!("提交说明只写一行。{format}"));
    }
    let Some((version, description)) = msg.strip_prefix('v').and_then(|rest| rest.split_once(": ")) else {
        return Err(format!("格式不对。{format}"));
    };
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("版本号要是 x.y.z 三段数字。{format}"));
    }
    let description = description.trim();
    if description.is_empty() || description.chars().count() > 120 {
        return Err(format!("description 要有，而且简约（120 个字以内）。{format}"));
    }
    Ok(format!("v{version}: {description}"))
}

fn git(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(root).env("GIT_TERMINAL_PROMPT", "0").env("GIT_LITERAL_PATHSPECS", "1").env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_INDEX_FILE");
    // 站主 shell 的 PATH：仓库的 pre-commit 钩子、git-lfs 这些要找得到（测试里不去读站主的 shell）。
    #[cfg(not(test))]
    if let Some((_, path)) = super::tools::bash::shell_env().into_iter().find(|(k, _)| k == "PATH") {
        c.env("PATH", path);
    }
    c
}

/// 跑一条 git，成功给标准输出；失败给它说的话。`stdin` 是喂给它的（`--pathspec-from-file=-`）。超过 [`TIMEOUT`] 就结束它。
fn run(mut command: Command, stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use std::io::Read;
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("起不了 git：{e}"))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(bytes);
    }
    // 两个输出各一个线程读，免得写满管道卡住；这边按时限等它退出。
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git 等了 {} 秒还没完（仓库的钩子卡住了？），停掉了。", TIMEOUT.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("git 出错了：{e}")),
        }
    };
    let (stdout, stderr) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if status.success() {
        return Ok(stdout);
    }
    let said = String::from_utf8_lossy(if stderr.is_empty() { &stdout } else { &stderr }).trim().to_string();
    Err(if said.is_empty() { "git 失败了。".into() } else { said })
}

/// 现在能不能提交：是 git 仓库、在某个分支上、没有做到一半的合并 / cherry-pick / revert。不能就给原因。
pub fn committable(root: &Path) -> Result<(), String> {
    if !is_repo(root) {
        return Err("这个项目不是 git 仓库，没提交。".into());
    }
    let mut head = git(root);
    head.args(["symbolic-ref", "-q", "HEAD"]);
    if run(head, None).is_err() {
        return Err("现在不在任何分支上（独立工作区，或者 HEAD 是游离的），没提交。".into());
    }
    for (mark, what) in [("MERGE_HEAD", "合并"), ("CHERRY_PICK_HEAD", "cherry-pick"), ("REVERT_HEAD", "revert")] {
        let mut c = git(root);
        c.args(["rev-parse", "-q", "--verify", mark]);
        if run(c, None).is_ok() {
            return Err(format!("仓库里有做到一半的{what}，没提交（也没动暂存区）。先把它做完或放弃。"));
        }
    }
    Ok(())
}

/// 这个文件夹在一个 git 仓库的工作区里。
pub fn is_repo(root: &Path) -> bool {
    let mut c = git(root);
    c.args(["rev-parse", "--is-inside-work-tree"]);
    run(c, None).is_ok_and(|out| out.starts_with(b"true"))
}

/// `root` 里没提交的改动（路径相对 `root`；改的、删的、新加的都算，被忽略的不算）。
pub fn changed(root: &Path) -> Result<Vec<String>, String> {
    // 项目可能是仓库里的一个子文件夹：`git status --porcelain` 给的路径相对仓库根，去掉这一截。
    let mut top = git(root);
    top.args(["rev-parse", "--show-prefix"]);
    let prefix = String::from_utf8_lossy(&run(top, None)?).trim().to_string();
    let mut c = git(root);
    // `--no-renames`：改名拆成一删一加，每条只有一个路径；`-z` 不转义路径。
    c.args(["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames", "--", "."]);
    let out = run(c, None)?;
    Ok(out
        .split(|b| *b == 0)
        .filter(|entry| entry.len() > 3)
        .filter_map(|entry| String::from_utf8_lossy(&entry[3..]).strip_prefix(&prefix).map(str::to_string))
        .collect())
}

/// `path` 是不是 `wanted` 里点了名的（就是它，或者在点名的文件夹里）。
fn named(path: &str, wanted: &[String]) -> bool {
    wanted.iter().any(|w| {
        let w = w.trim().trim_start_matches("./").trim_end_matches('/');
        w == "." || path == w || path.strip_prefix(w).is_some_and(|rest| rest.starts_with('/'))
    })
}

/// 提交。`paths` 空着就是全部没提交的改动；点了名的只提交其中确实有改动的那些。
pub fn commit(root: &Path, message: &str, paths: &[String]) -> Result<Commit, String> {
    let message = valid_message(message)?;
    committable(root)?;
    if let Some(bad) = paths.iter().find(|p| Path::new(p.trim()).is_absolute() || p.split(['/', '\\']).any(|part| part == "..")) {
        return Err(format!("{bad} 不是项目里的相对路径。"));
    }
    let all = changed(root)?;
    let files: Vec<String> = if paths.is_empty() { all } else { all.into_iter().filter(|p| named(p, paths)).collect() };
    if files.is_empty() {
        return Err(if paths.is_empty() { "工作区里没有没提交的改动，没提交。".into() } else { "点名的这些文件没有没提交的改动，没提交。".into() });
    }
    let spec: Vec<u8> = files.iter().flat_map(|f| f.bytes().chain(std::iter::once(0))).collect();
    let mut add = git(root);
    add.args(["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"]);
    run(add, Some(&spec))?;
    let mut commit = git(root);
    commit.args(["commit", "--only", "-m", &message, "--pathspec-from-file=-", "--pathspec-file-nul"]);
    run(commit, Some(&spec))?;
    let mut rev = git(root);
    rev.args(["rev-parse", "--short", "HEAD"]);
    let hash = String::from_utf8_lossy(&run(rev, None)?).trim().to_string();
    Ok(Commit { hash, message, files: files.len() })
}

/// 工具参数 → 提交 → 回给模型的一句。
pub fn call(root: &Path, input: &Value) -> Result<String, String> {
    let message = input.get("message").and_then(Value::as_str).unwrap_or("");
    let paths: Vec<String> = input.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let done = commit(root, message, &paths)?;
    Ok(format!("Committed {} ({} file{}): {}. Not pushed; the user pushes.", done.hash, done.files, if done.files == 1 { "" } else { "s" }, done.message))
}

/// 审批卡片上的一句和细节。
pub fn card(input: &Value) -> (String, String) {
    let message = input.get("message").and_then(Value::as_str).unwrap_or("").trim();
    let paths: Vec<&str> = input.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let what = if paths.is_empty() { "全部没提交的改动".to_string() } else { paths.join("\n") };
    (format!("提交：{message}"), format!("git commit（不 push）\n{what}"))
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::path::PathBuf;

    /// 一个临时 git 仓库：配了自己的 `user.name`，有一笔初始提交。
    pub fn repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("peck-git-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        init(&dir);
        dir
    }

    /// 把一个已有的文件夹变成 git 仓库（同上）。
    pub fn init(dir: &Path) {
        for args in [vec!["init", "-q", "-b", "main"], vec!["config", "user.name", "Site Owner"], vec!["config", "user.email", "owner@example.invalid"], vec!["config", "commit.gpgsign", "false"]] {
            let mut c = git(dir);
            c.args(&args);
            run(c, None).unwrap();
        }
        std::fs::write(dir.join("README.md"), "hello\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "ignored/\n").unwrap();
        commit(dir, "v0.0.1: initial release", &[]).unwrap();
    }

    /// 在项目里放一份装好的、要 `git_commit` 的插件（带一条收尾 Hook）。
    pub fn install_git_plan(dir: &Path) {
        let at = dir.join(".claude/plugins/git-plan");
        std::fs::create_dir_all(at.join(".claude-plugin")).unwrap();
        std::fs::create_dir_all(at.join("hooks")).unwrap();
        std::fs::write(at.join(".claude-plugin/plugin.json"), r#"{"name": "git-plan", "version": "1.0.0", "peck": {"title": "Git Plan", "tools": ["git_commit"]}}"#).unwrap();
        std::fs::write(at.join("hooks/hooks.json"), r#"{"hooks": {"Stop": [{"hooks": [{"type": "peck", "ask": "一行提交说明", "run": "git_commit"}]}]}}"#).unwrap();
    }

    pub fn commit_all(dir: &Path, message: &str) {
        commit(dir, message, &[]).unwrap();
    }

    pub fn log(dir: &Path, format: &str) -> Vec<String> {
        let mut c = git(dir);
        c.args(["log", &format!("--format={format}")]);
        String::from_utf8_lossy(&run(c, None).unwrap()).lines().map(str::to_string).collect()
    }

    pub fn files_in_head(dir: &Path) -> Vec<String> {
        let mut c = git(dir);
        c.args(["show", "--name-only", "--format=", "HEAD"]);
        let mut out: Vec<String> = String::from_utf8_lossy(&run(c, None).unwrap()).lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn messages_must_be_one_versioned_line_without_a_signature() {
        assert_eq!(valid_message("  v1.0.1: fix some problems \n").unwrap(), "v1.0.1: fix some problems");
        assert_eq!(valid_message("v12.3.40: minor improvement").unwrap(), "v12.3.40: minor improvement");
        for bad in ["", "fix some problems", "1.0.1: fix", "v1.0: fix", "v1.0.x: fix", "v1.0.1 fix", "v1.0.1:", "v1.0.1:    "] {
            assert!(valid_message(bad).is_err(), "{bad:?}");
        }
        assert!(valid_message("v1.0.1: fix\n\nmore detail").unwrap_err().contains("一行"));
        for signed in ["v1.0.1: fix\n\nCo-Authored-By: A <a@b.c>", "v1.0.1: fix 🤖 Generated with X", "v1.0.1: fix (signed-off-by: me)"] {
            assert!(valid_message(signed).unwrap_err().contains("署名"), "{signed:?}");
        }
        // 从模型的回答里取那一行：去掉代码块、反引号、引号、列表记号。
        assert_eq!(first_line("```\nv1.1.0: add plugins\n```"), "v1.1.0: add plugins");
        assert_eq!(first_line("\n- `v1.1.0: add plugins`\n解释一下……"), "v1.1.0: add plugins");
        assert_eq!(first_line("「v1.1.0: add plugins」"), "v1.1.0: add plugins");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn commits_only_what_was_named_and_never_signs() {
        let dir = repo("named");
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::write(dir.join("src/deep/b.txt"), "b\n").unwrap();
        std::fs::write(dir.join("mine.txt"), "站主自己改的\n").unwrap();
        std::fs::create_dir_all(dir.join("ignored")).unwrap();
        std::fs::write(dir.join("ignored/x.log"), "x\n").unwrap();
        // 站主自己 `git add` 过、这次没点名的：不带上。
        let mut add = git(&dir);
        add.args(["add", "mine.txt"]);
        run(add, None).unwrap();
        let mut seen = changed(&dir).unwrap();
        seen.sort();
        assert_eq!(seen, vec!["a.txt", "mine.txt", "src/deep/b.txt"], "被忽略的不算改动");

        let done = commit(&dir, "v1.0.1: fix some problems", &["a.txt".into(), "src".into(), "ignored".into(), "nope.txt".into()]).unwrap();
        assert_eq!((done.files, done.message.as_str()), (2, "v1.0.1: fix some problems"));
        assert_eq!(files_in_head(&dir), vec!["a.txt", "src/deep/b.txt"]);
        assert_eq!(log(&dir, "%s"), vec!["v1.0.1: fix some problems", "v0.0.1: initial release"]);
        // 用仓库自己配的名字；说明就是那一行，没有多出来的署名。
        assert_eq!(log(&dir, "%an <%ae>|%b")[0], "Site Owner <owner@example.invalid>|");
        assert_eq!(log(&dir, "%h")[0], done.hash);
        assert_eq!(changed(&dir).unwrap(), vec!["mine.txt"], "站主的那个还在工作区里等着");

        // 点名的没有改动、说明不合格、带署名：都不提交。
        assert!(commit(&dir, "v1.0.2: nothing", &["a.txt".into()]).unwrap_err().contains("没有没提交的改动"));
        assert!(commit(&dir, "update files", &[]).unwrap_err().contains("格式不对"));
        assert!(commit(&dir, "v1.0.2: x\n\nCo-Authored-By: Bot <b@x.y>", &[]).unwrap_err().contains("署名"));
        assert!(commit(&dir, "v1.0.2: x", &["../outside".into()]).unwrap_err().contains("相对路径"));
        assert_eq!(log(&dir, "%s").len(), 2);

        // 不点名：全部没提交的改动（删掉的文件也算）。
        std::fs::remove_file(dir.join("a.txt")).unwrap();
        let all = call(&dir, &json!({"message": "v1.0.2: minor improvement"})).unwrap();
        assert!(all.contains("2 files") && all.contains("Not pushed"), "{all}");
        assert_eq!(files_in_head(&dir), vec!["a.txt", "mine.txt"]);
        assert!(commit(&dir, "v1.0.3: nothing left", &[]).unwrap_err().contains("没有没提交的改动"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_outside_a_repo_and_on_a_detached_head() {
        let plain = std::env::temp_dir().join(format!("peck-git-plain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("a.txt"), "a").unwrap();
        assert!(!is_repo(&plain));
        assert!(commit(&plain, "v1.0.0: x", &[]).unwrap_err().contains("不是 git 仓库"));

        let dir = repo("detached");
        let mut detach = git(&dir);
        detach.args(["checkout", "-q", "--detach"]);
        run(detach, None).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        assert!(commit(&dir, "v1.0.1: x", &[]).unwrap_err().contains("不在任何分支上"));
        assert_eq!(log(&dir, "%s").len(), 1);

        // 合并到一半：在 `git add` 之前就拦下，暂存区不动。
        let merging = repo("merging");
        for args in [vec!["checkout", "-q", "-b", "other"], vec!["commit", "-q", "--allow-empty", "-m", "v0.0.2: other"], vec!["checkout", "-q", "main"], vec!["commit", "-q", "--allow-empty", "-m", "v0.0.2: main"], vec!["merge", "-q", "--no-ff", "--no-commit", "other"]] {
            let mut c = git(&merging);
            c.args(&args);
            run(c, None).unwrap();
        }
        std::fs::write(merging.join("a.txt"), "a").unwrap();
        assert!(committable(&merging).unwrap_err().contains("做到一半的合并"));
        assert!(commit(&merging, "v1.0.1: x", &[]).unwrap_err().contains("做到一半的合并"));
        let mut staged = git(&merging);
        staged.args(["diff", "--cached", "--name-only"]);
        assert!(run(staged, None).unwrap().is_empty(), "没提交就不动暂存区");
        let _ = std::fs::remove_dir_all(merging);

        // 路径按字面算：`[id]` 不当通配。
        let lit = repo("literal");
        std::fs::create_dir_all(lit.join("app/[id]")).unwrap();
        std::fs::create_dir_all(lit.join("app/i")).unwrap();
        std::fs::write(lit.join("app/[id]/page.tsx"), "a").unwrap();
        std::fs::write(lit.join("app/i/page.tsx"), "b").unwrap();
        commit(&lit, "v1.0.1: add page", &["app/[id]/page.tsx".into()]).unwrap();
        assert_eq!(files_in_head(&lit), vec!["app/[id]/page.tsx"]);
        assert_eq!(changed(&lit).unwrap(), vec!["app/i/page.tsx"]);
        let _ = std::fs::remove_dir_all(lit);

        // 项目是仓库里的一个子文件夹：只看、只提交这个文件夹里的。
        let mono = repo("mono");
        std::fs::create_dir_all(mono.join("app")).unwrap();
        std::fs::write(mono.join("app/a.txt"), "a").unwrap();
        std::fs::write(mono.join("outside.txt"), "o").unwrap();
        assert_eq!(changed(&mono.join("app")).unwrap(), vec!["a.txt"]);
        let done = commit(&mono.join("app"), "v1.0.1: add app", &[]).unwrap();
        assert_eq!(done.files, 1);
        assert_eq!(files_in_head(&mono), vec!["app/a.txt"]);
        for d in [plain, dir, mono] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
