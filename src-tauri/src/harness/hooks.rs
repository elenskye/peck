//! 插件的收尾 Hook（v1.1.0）：一轮做完，Peck 让刚干完的那个模型做一点必要的事（写提交说明），再自己跑程序（提交）。
//!
//! 现在只认一种：`Stop` 事件、`"type": "peck"`、`"run": "git_commit"`（Git Plan 的）。任意脚本的 Hook 不跑。
//!
//! 提交哪些（[`pending`]）：任务做完了（失败、打断、到上限的不跑）、不是 PLAN、不是独立工作区；这一轮改到的每个 git 仓库
//! （主文件夹和能改的其他文件夹各在的仓库，v1.2.1）各一笔，只提交这一轮改过、还没提交的文件，没改到的不碰。
//! 仓库根目录的 `CLAUDE.md` / `AGENTS.md` 里写了 Git 规则的照它的（`git::rules`），没写的照插件的（`ask`，`git::valid_message` 校验）。
//!
//! 怎么问：Peck Loop 在原对话末尾多问一轮（同样的系统提示和对话，走缓存，不显示成回答，见 `loop.rs`）；CLI 引擎续上它的会话、
//! 只读地问一句。一个仓库一段（`[编号]` 开头）。结果每个仓库发一条 `hook.ran`，界面写在这一轮的 recap 里。不弹卡片、不 push。

use super::git;
use crate::gateway::event::{EventBody, PermissionLevel};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, Control};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// CLI 续上会话问那一句，最多等多久。
const ASK_TIMEOUT: Duration = Duration::from_secs(180);

/// 项目里装着的插件带的收尾 Hook。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopHook {
    /// 哪个插件的（界面上的名字）。
    pub plugin: String,
    /// 没写 Git 规则的仓库照这一句写提交说明。
    pub ask: String,
}

/// 收尾要提交的一个仓库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub root: PathBuf,
    /// 主文件夹在的那个。
    pub main: bool,
    pub rules: Option<git::Rules>,
    /// 这一轮改过、还没提交的（相对仓库根）。Peck Loop 收尾前问的时候还不知道，是空的。
    pub files: Vec<String>,
}

/// 模型的回答：问的是哪几个仓库（照编号）、答了什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub roots: Vec<PathBuf>,
    pub text: String,
}

impl Answer {
    /// 这个仓库那一段：`[编号]` 那一行以下到下一个编号；只问了一个仓库、又没写编号的，整段都是。
    fn part(&self, root: &Path) -> Option<String> {
        let n = self.roots.iter().position(|r| r == root)? + 1;
        let mut current = None;
        let mut out: Vec<&str> = Vec::new();
        let mut marked = false;
        for line in self.text.lines() {
            let t = line.trim();
            let number = t.strip_prefix('[').and_then(|r| r.split_once(']')).and_then(|(k, rest)| k.trim().parse::<usize>().ok().map(|k| (k, rest)));
            if let Some((k, rest)) = number {
                marked = true;
                current = Some(k);
                let rest = rest.trim();
                if k == n && !rest.is_empty() && !rest.contains(&*root.to_string_lossy()) && !rest.contains("照 Git Plan") && !rest.contains("照这个仓库") {
                    out.push(rest);
                }
            } else if current == Some(n) && !t.starts_with("```") {
                out.push(line);
            }
        }
        if !marked && self.roots.len() == 1 {
            out = self.text.lines().filter(|l| !l.trim().starts_with("```")).collect();
        }
        let text = out.join("\n").trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

/// Peck Loop 用的：问哪一句、答案放哪（循环在收尾时问，任务收尾拍完快照以后取）。
#[derive(Clone, Default)]
pub struct StopAsk {
    pub ask: String,
    pub answer: Arc<Mutex<Option<Answer>>>,
}

impl StopAsk {
    pub fn of(hook: &Option<StopHook>) -> Option<Self> {
        hook.as_ref().map(|h| Self { ask: h.ask.clone(), answer: Arc::default() })
    }

    pub fn take(&self) -> Option<Answer> {
        self.answer.lock().ok().and_then(|mut a| a.take())
    }
}

/// 给模型看的那一句：包一层，说明这是 Peck 在问、不是站主的新要求。
pub fn question(ask: &str) -> String {
    format!("<peck-hook>\nThis is Peck (the harness), not the user. The task is finished; do not continue it and do not call tools. Answer only what is asked below, in plain text.\n\n{ask}\n</peck-hook>")
}

/// 要这几个仓库的提交说明：一个仓库一段，没写规则的照插件的 `ask`，写了的照它自己的（原文贴在后面，模型这时不能读文件）。
pub fn ask_for(ask: &str, repos: &[Repo]) -> String {
    let mut out = String::from("这一轮做完了，Peck 要替你提交这一轮改过的文件：改到的每个 git 仓库一笔，不 push。每个仓库回一段：单独一行写 [编号]，下面写这个仓库的提交说明。不要别的话，不要调用工具。\n");
    for (i, r) in repos.iter().enumerate() {
        let how = if r.rules.is_some() { "照这个仓库自己的 Git 规则" } else { "照 Git Plan" };
        out.push_str(&format!("\n[{}] {}：{how}", i + 1, r.root.display()));
    }
    if repos.iter().any(|r| r.rules.is_none()) {
        out.push_str(&format!("\n\nGit Plan：{ask}"));
    }
    for r in repos {
        if let Some(rules) = &r.rules {
            let from: Vec<String> = rules.files.iter().map(|f| f.display().to_string()).collect();
            out.push_str(&format!("\n\n{} 的 Git 规则（{}）：\n{}", r.root.display(), from.join("、"), rules.text));
        }
    }
    out
}

/// 收尾要看的文件夹：主文件夹在前，再是能改的其他文件夹（PLAN 不看其他文件夹）。
fn repos_of(folders: &[(PathBuf, Vec<String>)]) -> Vec<Repo> {
    let mut out: Vec<Repo> = Vec::new();
    for (i, (folder, files)) in folders.iter().enumerate() {
        let Some((root, prefix)) = git::top(folder) else { continue };
        if git::committable(&root).is_err() {
            continue;
        }
        let files = files.iter().map(|f| format!("{prefix}{f}"));
        match out.iter_mut().find(|r| r.root == root) {
            Some(r) => r.files.extend(files),
            None => out.push(Repo { rules: git::rules(&root), main: i == 0, files: files.collect(), root }),
        }
    }
    out
}

/// 任务开头记一下（Peck Loop 用）：每个文件夹这时候已经有哪些没提交的改动（不能提交的是 `None`，收尾时不问它）。
pub fn baseline(folders: &[PathBuf]) -> Vec<(PathBuf, Option<Vec<String>>)> {
    folders
        .iter()
        .map(|f| (f.clone(), git::top(f).filter(|(root, _)| git::committable(root).is_ok()).and_then(|_| git::changed(f).ok())))
        .collect()
}

/// Peck Loop 收尾前：要问哪几个仓库。开头能提交、而且这一次确实动了它——没提交的改动和开头不一样了，或者自己的工具改过
/// 那里的文件（`touched`，绝对路径；改的是开头就没提交的文件时前一条看不出来）。光是本来就不干净不算。
pub fn asking(baseline: &[(PathBuf, Option<Vec<String>>)], touched: &[PathBuf]) -> Vec<Repo> {
    let Some((main, _)) = baseline.first() else { return vec![] };
    let moved: Vec<(PathBuf, Vec<String>)> = baseline
        .iter()
        .filter(|(folder, before)| {
            let (Some(before), Ok(now)) = (before, git::changed(folder)) else { return false };
            !now.is_empty() && (now != *before || touched.iter().any(|t| t.starts_with(folder)))
        })
        .map(|(folder, _)| (folder.clone(), vec![]))
        .collect();
    let main_root = git::top(main).map(|(r, _)| r);
    let mut repos = repos_of(&moved);
    for r in &mut repos {
        r.main = Some(&r.root) == main_root.as_ref();
    }
    repos
}

/// 这一轮要提交的仓库（`folders`：主文件夹在前，每个带这一轮改过的文件，相对那个文件夹）。
pub fn pending(folders: &[(PathBuf, Vec<String>)], level: PermissionLevel, succeeded: bool, worktree: bool) -> Vec<Repo> {
    if !succeeded || worktree || level == PermissionLevel::Plan {
        return vec![];
    }
    let mut repos = repos_of(folders);
    for r in &mut repos {
        let uncommitted = git::changed(&r.root).unwrap_or_default();
        r.files.retain(|f| uncommitted.contains(f));
        r.files.sort();
        r.files.dedup();
    }
    repos.retain(|r| !r.files.is_empty());
    repos
}

/// 用模型答的那一段提交这个仓库；给回要发的那条 `hook.ran`。
pub fn commit(hook: &StopHook, repo: &Repo, answer: Option<&Answer>) -> EventBody {
    // 照 Git Plan 的取第一行合格的（模型可能先抄一遍编号那一行）。
    let message = answer.and_then(|a| a.part(&repo.root)).map(|m| match repo.rules {
        Some(_) => m,
        None => m.lines().map(git::first_line).find(|l| git::valid_message(l).is_ok()).unwrap_or_else(|| git::first_line(&m)),
    });
    let result = match message.filter(|m| !m.is_empty()) {
        None => Err("模型没给出提交说明。".to_string()),
        Some(m) if repo.rules.is_some() => git::commit_as_is(&repo.root, &m, &repo.files),
        Some(m) => git::commit(&repo.root, &m, &repo.files),
    };
    let at = if repo.main { String::new() } else { format!("在 {} ", repo.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) };
    let (ok, text) = match result {
        Ok(done) => (true, format!("{} {at}提交了 {}：{}（{} 个文件，没有 push）", hook.plugin, done.hash, done.message.lines().next().unwrap_or(""), done.files)),
        Err(why) => (false, format!("{} {at}没提交：{why}", hook.plugin)),
    };
    EventBody::HookRan { plugin: hook.plugin.clone(), action: git::GIT_COMMIT.into(), ok, text, event: None }
}

/// 任务收尾（快照拍完以后）：有要提交的仓库，就去拿提交说明（`answer`，只在要提交时才叫，给它这几个仓库）、各提交一笔。
/// 给回要发的 `hook.ran`（一个仓库一条）。
pub async fn finish<F, Fut>(hook: Option<&StopHook>, folders: Vec<(PathBuf, Vec<String>)>, level: PermissionLevel, succeeded: bool, worktree: bool, answer: F) -> Vec<EventBody>
where
    F: FnOnce(Vec<Repo>) -> Fut,
    Fut: std::future::Future<Output = Option<Answer>>,
{
    let Some(hook) = hook.cloned() else { return vec![] };
    let repos = tokio::task::spawn_blocking(move || pending(&folders, level, succeeded, worktree)).await.unwrap_or_default();
    if repos.is_empty() {
        return vec![];
    }
    let answer = answer(repos.clone()).await;
    tokio::task::spawn_blocking(move || repos.iter().map(|r| commit(&hook, r, answer.as_ref())).collect()).await.unwrap_or_default()
}

/// CLI 引擎：续上它刚才的会话（`task.resume`），只读地问那一句，拿它最后一段回答。事件不进这个任务的记录、不显示；
/// 用量照样转给 `forward`（算进花费）。它要审批、要问站主就打断（不该再动手）；超时也打断。直接调用的引擎、续不了的不问。
pub async fn ask_cli(backend: &dyn AgentBackend, engine: &str, mut task: AgentTask, ask: &str, forward: impl Fn(EventBody) + Send + Sync + 'static) -> Option<String> {
    let caps = backend.caps();
    if !caps.resume || task.resume.is_none() {
        return None;
    }
    task.prompt = question(ask);
    task.permission = PermissionLevel::Plan;
    task.peck_mcp = None;
    task.run_id = format!("{}-hook", task.run_id);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let said: Arc<Mutex<String>> = Arc::default();
    let (heard, stop) = (said.clone(), tx.clone());
    let sink = EventSink::new(task.run_id.clone(), None, engine, None, move |e: &crate::gateway::event::AgentEvent| match &e.body {
        EventBody::MessageCompleted { text, .. } if !text.trim().is_empty() => {
            if let Ok(mut s) = heard.lock() {
                *s = text.clone();
            }
        }
        EventBody::UsageUpdated { .. } => forward(e.body.clone()),
        EventBody::ApprovalRequested { .. } | EventBody::UserInputRequested { .. } => {
            let _ = stop.send(Control::Interrupt);
        }
        _ => {}
    });
    let run = backend.run(task, sink, rx);
    tokio::pin!(run);
    if tokio::time::timeout(ASK_TIMEOUT, &mut run).await.is_err() {
        let _ = tx.send(Control::Interrupt);
        let _ = tokio::time::timeout(Duration::from_secs(15), &mut run).await;
    }
    let text = said.lock().ok().map(|s| s.trim().to_string()).unwrap_or_default();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git::testing as g;

    fn said(body: EventBody) -> (bool, String) {
        match body {
            EventBody::HookRan { ok, text, plugin, action, .. } if plugin == "Git Plan" && action == "git_commit" => (ok, text),
            other => panic!("{other:?}"),
        }
    }

    /// 假 CLI：记下收到的任务，照剧本出几条事件。
    struct FakeCli {
        caps: crate::gateway::caps::AgentCaps,
        events: Vec<EventBody>,
        seen: Arc<Mutex<Vec<AgentTask>>>,
        /// 发完事件以后等打断才结束（它在等审批）。
        waits: bool,
    }

    impl AgentBackend for FakeCli {
        fn caps(&self) -> crate::gateway::caps::AgentCaps {
            self.caps.clone()
        }

        fn run(&self, task: AgentTask, sink: EventSink, mut control: tokio::sync::mpsc::UnboundedReceiver<Control>) -> crate::gateway::BoxFuture<Result<crate::gateway::RunOutcome, crate::gateway::GatewayError>> {
            self.seen.lock().unwrap().push(task);
            let (events, waits) = (self.events.clone(), self.waits);
            Box::pin(async move {
                for e in events {
                    sink.emit(e);
                }
                let mut reason = crate::gateway::event::TurnEndReason::Done;
                if waits {
                    while let Some(c) = control.recv().await {
                        if matches!(c, Control::Interrupt) {
                            reason = crate::gateway::event::TurnEndReason::Interrupted;
                            break;
                        }
                    }
                }
                Ok(crate::gateway::RunOutcome { reason, session_ref: Some("s2".into()) })
            })
        }
    }

    fn task(resume: Option<&str>) -> AgentTask {
        AgentTask { run_id: "run-9".into(), prompt: "原来的话".into(), cwd: std::env::temp_dir(), permission: PermissionLevel::Auto, model: None, effort: None, resume: resume.map(str::to_string), peck_mcp: None, env: vec![], folders: vec![], compact: false, trusted: false }
    }

    #[tokio::test]
    async fn a_cli_is_asked_read_only_on_its_own_session_and_stays_out_of_the_log() {
        use crate::gateway::caps::AgentCaps;
        use crate::gateway::event::{Cost, CostSource, TokenUsage, ToolKind, UsageScope};
        let usage = EventBody::UsageUpdated { usage: TokenUsage::default(), cost: Cost { usd: None, source: CostSource::Unknown }, scope: UsageScope::Turn, model: None };
        let reply = |text: &str| EventBody::MessageCompleted { item_id: "m1".into(), text: text.into() };
        let seen: Arc<Mutex<Vec<AgentTask>>> = Arc::default();
        let forwarded: Arc<Mutex<Vec<EventBody>>> = Arc::default();
        let forward = |to: Arc<Mutex<Vec<EventBody>>>| move |body: EventBody| to.lock().unwrap().push(body);
        // 续上刚才的会话、只读、不挂 Peck 的工具；只把用量转出来，回答不进记录。
        let cli = FakeCli { caps: AgentCaps::codex(), events: vec![reply("先想一下"), usage.clone(), reply("v1.0.1: fix some problems")], seen: seen.clone(), waits: false };
        let answer = ask_cli(&cli, "codex", task(Some("s1")), "一行提交说明", forward(forwarded.clone())).await;
        assert_eq!(answer.as_deref(), Some("v1.0.1: fix some problems"));
        let got = seen.lock().unwrap()[0].clone();
        assert_eq!((got.resume.as_deref(), got.permission, got.run_id.as_str()), (Some("s1"), PermissionLevel::Plan, "run-9-hook"));
        assert!(got.prompt.contains("<peck-hook>") && got.prompt.contains("一行提交说明") && !got.prompt.contains("原来的话"));
        assert!(got.peck_mcp.is_none());
        assert_eq!(forwarded.lock().unwrap().len(), 1);
        assert!(matches!(forwarded.lock().unwrap()[0], EventBody::UsageUpdated { .. }));
        // 它想动手（要审批）：打断，不等；没答出来就是没有。
        let wants = EventBody::ApprovalRequested { reason: None, request_id: "a1".into(), call_id: None, kind: ToolKind::Run, title: "跑 git commit".into(), detail: None, high_risk: false, options: vec![] };
        let cli = FakeCli { caps: AgentCaps::codex(), events: vec![wants], seen: seen.clone(), waits: true };
        assert_eq!(ask_cli(&cli, "codex", task(Some("s1")), "一行", forward(forwarded.clone())).await, None);
        // 没有会话号可续：不问，引擎都不起。
        let before = seen.lock().unwrap().len();
        let cli = FakeCli { caps: AgentCaps::codex(), events: vec![reply("v1.0.1: x")], seen: seen.clone(), waits: false };
        assert_eq!(ask_cli(&cli, "codex", task(None), "一行", forward(forwarded.clone())).await, None);
        assert_eq!(seen.lock().unwrap().len(), before);
    }

    fn answer(roots: &[&Path], text: &str) -> Answer {
        Answer { roots: roots.iter().map(|r| r.to_path_buf()).collect(), text: text.into() }
    }

    #[tokio::test]
    async fn only_a_finished_round_with_uncommitted_changes_is_committed() {
        let dir = g::repo("hook");
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.join("mine.txt"), "站主自己改的\n").unwrap();
        let round = |files: &[&str]| vec![(dir.clone(), files.iter().map(|f| f.to_string()).collect::<Vec<_>>())];
        // Peck Loop 值不值得问：开头就这样、这一次没动过的不问；动了才问。
        let before = baseline(std::slice::from_ref(&dir));
        assert!(asking(&before, &[]).is_empty());
        assert_eq!(asking(&before, &[dir.join("a.txt")]).len(), 1);
        assert_eq!(asking(&[(dir.clone(), Some(vec![]))], &[]).len(), 1);
        assert!(asking(&[(dir.clone(), None)], &[dir.join("a.txt")]).is_empty());
        // 这一轮改过、现在还没提交的才算。
        let repos = pending(&round(&["a.txt", "gone.txt"]), PermissionLevel::Edit, true, false);
        assert_eq!(repos.len(), 1);
        assert!(repos[0].main && repos[0].rules.is_none() && repos[0].files == vec!["a.txt".to_string()]);
        // 没做完、PLAN、独立工作区、这一轮没改文件：都不提交。
        assert!(pending(&round(&["a.txt"]), PermissionLevel::Edit, false, false).is_empty());
        assert!(pending(&round(&["a.txt"]), PermissionLevel::Plan, true, false).is_empty());
        assert!(pending(&round(&["a.txt"]), PermissionLevel::Auto, true, true).is_empty());
        assert!(pending(&round(&[]), PermissionLevel::Auto, true, false).is_empty());
        assert!(pending(&round(&["mine-other.txt"]), PermissionLevel::Auto, true, false).is_empty());

        let hook = StopHook { plugin: "Git Plan".into(), ask: "一行提交说明".into() };
        let repo = &repos[0];
        // 没答、答得不合格式：不提交，写明原因。
        let none = said(commit(&hook, repo, None));
        assert!(!none.0 && none.1.contains("Git Plan 没提交：模型没给出提交说明"));
        let bad = said(commit(&hook, repo, Some(&answer(&[&dir], "改了几个文件"))));
        assert!(!bad.0 && bad.1.contains("格式不对"));
        assert_eq!(g::log(&dir, "%s").len(), 1);
        // 合格：只提交这一轮的文件，站主自己改的留着；不署名。只问了一个仓库、没写编号也认。
        let ok = said(commit(&hook, repo, Some(&answer(&[&dir], "```\nv1.0.1: fix some problems\n```\n（解释……）"))));
        assert_eq!(g::log(&dir, "%s|%b")[0], "v1.0.1: fix some problems|");
        assert_eq!(g::files_in_head(&dir), vec!["a.txt"]);
        assert_eq!(git::changed(&dir).unwrap(), vec!["mine.txt"]);
        assert!(ok.0 && ok.1.starts_with("Git Plan 提交了 ") && ok.1.contains("v1.0.1: fix some problems（1 个文件，没有 push）"), "{ok:?}");
        // `finish`：该提交才去拿提交说明（不该提交时不问）；提交完给回那条事件。
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        let asked = Arc::new(Mutex::new(0));
        let ask = |n: Arc<Mutex<i32>>| move |repos: Vec<Repo>| async move {
            *n.lock().unwrap() += 1;
            Some(Answer { roots: repos.iter().map(|r| r.root.clone()).collect(), text: "[1]\nv1.0.2: minor improvement".into() })
        };
        assert!(finish(Some(&hook), round(&["b.txt"]), PermissionLevel::Plan, true, false, ask(asked.clone())).await.is_empty());
        assert!(finish(Some(&hook), round(&["b.txt"]), PermissionLevel::Edit, false, false, ask(asked.clone())).await.is_empty());
        assert!(finish(None, round(&["b.txt"]), PermissionLevel::Edit, true, false, ask(asked.clone())).await.is_empty());
        assert_eq!(*asked.lock().unwrap(), 0);
        let done = finish(Some(&hook), round(&["b.txt"]), PermissionLevel::Edit, true, false, ask(asked.clone())).await;
        assert!(done.len() == 1 && said(done[0].clone()).0 && *asked.lock().unwrap() == 1);
        assert_eq!(g::files_in_head(&dir), vec!["b.txt"]);
        // 提交过了：这一轮没有要提交的了。
        assert!(pending(&round(&["a.txt", "b.txt"]), PermissionLevel::Edit, true, false).is_empty());
        assert!(question("x").contains("<peck-hook>") && question("一行").contains("一行"));
        // 不是 git 仓库：不问、不提交。
        let plain = std::env::temp_dir().join(format!("peck-hook-plain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("a.txt"), "a").unwrap();
        assert_eq!(baseline(std::slice::from_ref(&plain)), vec![(plain.clone(), None)]);
        assert!(pending(&[(plain.clone(), vec!["a.txt".to_string()])], PermissionLevel::Auto, true, false).is_empty());
        for d in [dir, plain] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// 新项目（主文件夹，没写 Git 规则）+ Atelier（其他文件夹，自己的 CLAUDE.md 有 Git 一节）：各提交一笔，各照各的；没改到的仓库不碰。
    #[test]
    fn each_changed_repo_gets_its_own_commit_by_its_own_rules() {
        let main = g::repo("hook-main");
        let atelier = g::repo("hook-atelier");
        let idle = g::repo("hook-idle");
        std::fs::write(atelier.join("CLAUDE.md"), "# Atelier\n\n## 版本与 Git\n\n- 提交信息：`vx.x.x: dev description`，写当前的仓库版本。\n\n```sh\n# 不是标题\n```\n\n## 别的\n\n不相干。\n").unwrap();
        std::fs::write(atelier.join("AGENTS.md"), "见 CLAUDE.md。\n").unwrap();
        g::commit_all(&atelier, "v0.0.2: rules");
        std::fs::write(main.join("app.txt"), "app\n").unwrap();
        std::fs::write(atelier.join("canvas.txt"), "canvas\n").unwrap();
        std::fs::write(idle.join("dirty.txt"), "站主自己的\n").unwrap();
        let folders = vec![(main.clone(), vec!["app.txt".to_string()]), (atelier.clone(), vec!["canvas.txt".to_string()]), (idle.clone(), vec![])];
        let repos = pending(&folders, PermissionLevel::Auto, true, false);
        assert_eq!(repos.iter().map(|r| r.root.clone()).collect::<Vec<_>>(), vec![main.clone(), atelier.clone()]);
        assert!(repos[0].main && repos[0].rules.is_none());
        let rules = repos[1].rules.clone().unwrap();
        assert!(!repos[1].main && rules.files == vec![atelier.join("CLAUDE.md")]);
        assert!(rules.text.starts_with("## 版本与 Git") && rules.text.contains("# 不是标题") && !rules.text.contains("不相干"), "{}", rules.text);
        // 问的那一句：一个仓库一个编号，Git Plan 的规矩、Atelier 的原文都在。
        let hook = StopHook { plugin: "Git Plan".into(), ask: "只写一行 v<版本>: <description>".into() };
        let q = ask_for(&hook.ask, &repos);
        assert!(q.contains(&format!("[1] {}：照 Git Plan", main.display())) && q.contains(&format!("[2] {}：照这个仓库自己的 Git 规则", atelier.display())));
        assert!(q.contains("Git Plan：只写一行") && q.contains("`vx.x.x: dev description`"));
        // 各提交一笔：主仓库照 Git Plan 校验，Atelier 照原样（可以有正文）。
        let reply = answer(&[&main, &atelier], &format!("[1] {}\n{}：照 Git Plan\nv1.0.1: add app\n\n[2] 照这个仓库自己的 Git 规则\n```\nv0.0.3: add canvas\n\nbody line\n```", main.display(), main.display()));
        let events: Vec<(bool, String)> = repos.iter().map(|r| said(commit(&hook, r, Some(&reply)))).collect();
        assert!(events[0].0 && events[0].1.starts_with("Git Plan 提交了") && events[0].1.contains("v1.0.1: add app"), "{events:?}");
        assert!(events[1].0 && events[1].1.contains(&format!("在 {} 提交了", atelier.file_name().unwrap().to_string_lossy())) && events[1].1.contains("v0.0.3: add canvas（1 个文件"), "{events:?}");
        assert_eq!(g::log(&main, "%s")[0], "v1.0.1: add app");
        assert_eq!(g::log(&atelier, "%B")[0], "v0.0.3: add canvas");
        assert_eq!(g::files_in_head(&atelier), vec!["canvas.txt"]);
        assert_eq!(g::log(&idle, "%s").len(), 1, "没改到的仓库不碰");
        assert_eq!(git::changed(&idle).unwrap(), vec!["dirty.txt"]);
        // 少答了一个：那个仓库不提交，写明原因。
        std::fs::write(atelier.join("canvas.txt"), "canvas 2\n").unwrap();
        let repos = pending(&[(atelier.clone(), vec!["canvas.txt".to_string()])], PermissionLevel::Auto, true, false);
        let missing = said(commit(&hook, &repos[0], Some(&answer(&[&main, &atelier], "[1]\nv1.0.2: x"))));
        assert!(!missing.0 && missing.1.contains("模型没给出提交说明"), "{missing:?}");
        for d in [main, atelier, idle] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
