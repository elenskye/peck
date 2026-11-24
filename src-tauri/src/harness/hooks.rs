//! 插件的收尾 Hook（v1.1.0）：一轮做完，Peck 让刚干完的那个模型做一点必要的事（写一行提交说明），再自己跑程序（提交）。
//!
//! 现在只认一种：`Stop` 事件、`"type": "peck"`、`"run": "git_commit"`（Git Plan 的）。任意脚本的 Hook 不跑。
//!
//! 什么时候跑（[`pending`]）：任务做完了（失败、打断、到上限的不跑）、不是 PLAN、不是独立工作区、项目是 git 仓库且在某个
//! 分支上、这一轮改过的文件里还有没提交的。
//!
//! 怎么问：Peck Loop 在原对话末尾多问一轮（同样的系统提示和对话，走缓存，不显示成回答，见 `loop.rs`）；CLI 引擎续上它的会话、
//! 只读地问一句。答回来的第一行当提交说明，过 `git::valid_message`；合格就只提交这一轮改过的文件（[`commit`]），
//! 不合格、没答就不提交。结果发一条 `hook.ran`，界面写在这一轮的 recap 里。不弹卡片、不 push。

use super::git;
use crate::gateway::event::{EventBody, PermissionLevel};
use crate::gateway::log::EventSink;
use crate::gateway::{AgentBackend, AgentTask, Control};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// CLI 续上会话问那一句，最多等多久。
const ASK_TIMEOUT: Duration = Duration::from_secs(180);

/// 项目里装着的插件带的收尾 Hook。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopHook {
    /// 哪个插件的（界面上的名字）。
    pub plugin: String,
    /// 问刚干完的模型的那一句。
    pub ask: String,
}

/// Peck Loop 用的：问哪一句、答案放哪（循环在收尾时问，任务收尾拍完快照以后取）。
#[derive(Clone, Default)]
pub struct StopAsk {
    pub ask: String,
    pub answer: Arc<Mutex<Option<String>>>,
}

impl StopAsk {
    pub fn of(hook: &Option<StopHook>) -> Option<Self> {
        hook.as_ref().map(|h| Self { ask: h.ask.clone(), answer: Arc::default() })
    }

    pub fn take(&self) -> Option<String> {
        self.answer.lock().ok().and_then(|mut a| a.take())
    }
}

/// 给模型看的那一句：包一层，说明这是 Peck 在问、不是站主的新要求。
pub fn question(ask: &str) -> String {
    format!("<peck-hook>\nThis is Peck (the harness), not the user. The task is finished; do not continue it and do not call tools. Answer only what is asked below, in plain text.\n\n{ask}\n</peck-hook>")
}

/// 任务开头记一下（Peck Loop 用）：能提交的话，这时候工作区里已经有哪些没提交的改动。不能提交（不是 git 仓库、不在分支上、
/// 合并到一半）就是 `None`，收尾时不问。
pub fn baseline(cwd: &Path) -> Option<Vec<String>> {
    git::committable(cwd).ok()?;
    git::changed(cwd).ok()
}

/// Peck Loop 收尾前：这一次值不值得问。任务开头能提交，而且这一次确实动了东西——没提交的改动和开头不一样了，
/// 或者自己的工具改过文件（`touched`；改的是开头就没提交的文件时前一条看不出来）。光是工作区本来就不干净不算，
/// 不然只问不改的任务每次都白问一轮。
pub fn wants_ask(cwd: &Path, baseline: Option<&[String]>, touched: bool) -> bool {
    let Some(before) = baseline else { return false };
    let Ok(now) = git::changed(cwd) else { return false };
    !now.is_empty() && (touched || now != before)
}

/// 这一轮要不要提交：要的话给回要提交的文件（这一轮改过、现在还没提交的）。
pub fn pending(cwd: &Path, level: PermissionLevel, succeeded: bool, worktree: bool, round_files: &[String]) -> Option<Vec<String>> {
    if !succeeded || worktree || level == PermissionLevel::Plan || round_files.is_empty() || git::committable(cwd).is_err() {
        return None;
    }
    let uncommitted = git::changed(cwd).ok()?;
    let files: Vec<String> = round_files.iter().filter(|f| uncommitted.contains(f)).cloned().collect();
    (!files.is_empty()).then_some(files)
}

/// 用模型答的那一行提交这些文件；给回要发的那条 `hook.ran`。
pub fn commit(hook: &StopHook, cwd: &Path, files: &[String], answer: Option<&str>) -> EventBody {
    let result = match answer.map(git::first_line).filter(|line| !line.is_empty()) {
        None => Err("模型没给出提交说明。".to_string()),
        Some(line) => git::commit(cwd, &line, files),
    };
    let (ok, text) = match result {
        Ok(done) => (true, format!("{} 提交了 {}：{}（{} 个文件，没有 push）", hook.plugin, done.hash, done.message, done.files)),
        Err(why) => (false, format!("{} 没提交：{why}", hook.plugin)),
    };
    EventBody::HookRan { plugin: hook.plugin.clone(), action: git::GIT_COMMIT.into(), ok, text, event: None }
}

/// 任务收尾（快照拍完以后）：这一轮该提交，就去拿提交说明（`answer`，只在要提交时才叫）、提交。给回要发的 `hook.ran`。
pub async fn finish<F, Fut>(hook: Option<&StopHook>, cwd: &Path, level: PermissionLevel, succeeded: bool, worktree: bool, round_files: Vec<String>, answer: F) -> Option<EventBody>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let hook = hook?.clone();
    let at = cwd.to_path_buf();
    let files = tokio::task::spawn_blocking(move || pending(&at, level, succeeded, worktree, &round_files)).await.ok().flatten()?;
    let answer = answer().await;
    let at = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || commit(&hook, &at, &files, answer.as_deref())).await.ok()
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
        AgentTask { run_id: "run-9".into(), prompt: "原来的话".into(), cwd: std::env::temp_dir(), permission: PermissionLevel::Auto, model: None, effort: None, resume: resume.map(str::to_string), peck_mcp: None, env: vec![], folders: vec![], compact: false }
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
        let wants = EventBody::ApprovalRequested { request_id: "a1".into(), call_id: None, kind: ToolKind::Run, title: "跑 git commit".into(), detail: None, high_risk: false, options: vec![] };
        let cli = FakeCli { caps: AgentCaps::codex(), events: vec![wants], seen: seen.clone(), waits: true };
        assert_eq!(ask_cli(&cli, "codex", task(Some("s1")), "一行", forward(forwarded.clone())).await, None);
        // 没有会话号可续：不问，引擎都不起。
        let before = seen.lock().unwrap().len();
        let cli = FakeCli { caps: AgentCaps::codex(), events: vec![reply("v1.0.1: x")], seen: seen.clone(), waits: false };
        assert_eq!(ask_cli(&cli, "codex", task(None), "一行", forward(forwarded.clone())).await, None);
        assert_eq!(seen.lock().unwrap().len(), before);
    }

    #[tokio::test]
    async fn only_a_finished_round_with_uncommitted_changes_is_committed() {
        let dir = g::repo("hook");
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.join("mine.txt"), "站主自己改的\n").unwrap();
        let round = vec!["a.txt".to_string(), "gone.txt".to_string()];
        // Peck Loop 值不值得问：开头就这样、这一次没动过的不问；动了才问。
        let before = baseline(&dir).unwrap();
        assert!(!wants_ask(&dir, Some(&before), false));
        assert!(wants_ask(&dir, Some(&before), true));
        assert!(wants_ask(&dir, Some(&[]), false));
        assert!(!wants_ask(&dir, None, true));
        // 这一轮改过、现在还没提交的才算。
        assert_eq!(pending(&dir, PermissionLevel::Edit, true, false, &round), Some(vec!["a.txt".to_string()]));
        // 没做完、PLAN、独立工作区、这一轮没改文件：都不提交。
        assert_eq!(pending(&dir, PermissionLevel::Edit, false, false, &round), None);
        assert_eq!(pending(&dir, PermissionLevel::Plan, true, false, &round), None);
        assert_eq!(pending(&dir, PermissionLevel::Auto, true, true, &round), None);
        assert_eq!(pending(&dir, PermissionLevel::Auto, true, false, &[]), None);
        assert_eq!(pending(&dir, PermissionLevel::Auto, true, false, &["mine-other.txt".to_string()]), None);

        let hook = StopHook { plugin: "Git Plan".into(), ask: "一行提交说明".into() };
        // 没答、答得不合格式：不提交，写明原因。
        let none = said(commit(&hook, &dir, &["a.txt".into()], None));
        assert!(!none.0 && none.1.contains("Git Plan 没提交：模型没给出提交说明"));
        let bad = said(commit(&hook, &dir, &["a.txt".into()], Some("改了几个文件")));
        assert!(!bad.0 && bad.1.contains("格式不对"));
        assert_eq!(g::log(&dir, "%s").len(), 1);
        // 合格：只提交这一轮的文件，站主自己改的留着；不署名。
        let ok = said(commit(&hook, &dir, &["a.txt".into()], Some("```\nv1.0.1: fix some problems\n```\n（解释……）")));
        assert_eq!(g::log(&dir, "%s|%b")[0], "v1.0.1: fix some problems|");
        assert_eq!(g::files_in_head(&dir), vec!["a.txt"]);
        assert_eq!(git::changed(&dir).unwrap(), vec!["mine.txt"]);
        assert!(ok.0 && ok.1.starts_with("Git Plan 提交了 ") && ok.1.contains("v1.0.1: fix some problems（1 个文件，没有 push）"), "{ok:?}");
        // `finish`：该提交才去拿提交说明（不该提交时不问）；提交完给回那条事件。
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        let asked = Arc::new(Mutex::new(0));
        let ask = |n: Arc<Mutex<i32>>| move || async move {
            *n.lock().unwrap() += 1;
            Some("v1.0.2: minor improvement".to_string())
        };
        assert!(finish(Some(&hook), &dir, PermissionLevel::Plan, true, false, vec!["b.txt".into()], ask(asked.clone())).await.is_none());
        assert!(finish(Some(&hook), &dir, PermissionLevel::Edit, false, false, vec!["b.txt".into()], ask(asked.clone())).await.is_none());
        assert!(finish(None, &dir, PermissionLevel::Edit, true, false, vec!["b.txt".into()], ask(asked.clone())).await.is_none());
        assert_eq!(*asked.lock().unwrap(), 0);
        let done = said(finish(Some(&hook), &dir, PermissionLevel::Edit, true, false, vec!["b.txt".into()], ask(asked.clone())).await.unwrap());
        assert!(done.0 && *asked.lock().unwrap() == 1);
        assert_eq!(g::files_in_head(&dir), vec!["b.txt"]);
        // 提交过了：这一轮没有要提交的了。
        assert_eq!(pending(&dir, PermissionLevel::Edit, true, false, &round), None);
        assert!(question("x").contains("<peck-hook>") && question("一行").contains("一行"));
        // 不是 git 仓库：不问、不提交。
        let plain = std::env::temp_dir().join(format!("peck-hook-plain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("a.txt"), "a").unwrap();
        assert!(baseline(&plain).is_none());
        assert_eq!(pending(&plain, PermissionLevel::Auto, true, false, &["a.txt".to_string()]), None);
        for d in [dir, plain] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
