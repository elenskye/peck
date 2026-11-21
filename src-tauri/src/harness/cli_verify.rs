//! CLI 也走后台验证（v0.10 Phase 15，roadmap C1）。
//!
//! CLI 引擎（和合作里 CLI 实施、按审查改的那一段）做完以后，Peck 按项目 `.peck/verify.toml` 里站主确认过的检查跑一遍：
//! 和 Peck 循环同一套 [`Monitor`]（同样的沙箱、超时；平时只跑改过的文件管得到的那几条，目标模式是目标那几条）。
//! 都过了挂 ✓；没过就用同一个 CLI 续接同一个会话，把失败的输出交回去修，规则和 Peck 循环一样（[`FixGate`]：最多 3 次、
//! 目标模式 8 次，同一个失败连续两次、两次之间没改文件都停）。
//! Antigravity 只做直接调用：只跑一遍、显示结果，不交回去。CLI 自己的会话续接不了的，也只跑一遍。

use super::monitor::{self, FixGate, Monitor};
use super::permission;
use super::r#loop::{FIX_ATTEMPTS, GOAL_ATTEMPTS};
use super::tools::ToolContext;
use super::verify::Check;
use crate::gateway::event::{EventBody, PermissionLevel, VerifyOutcome, VerifyTrigger};
use crate::gateway::log::EventSink;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;

/// 这一轮做完以后怎么办。
#[derive(Debug, Clone, PartialEq)]
pub enum After {
    /// 收尾：都过了、没有要看的，或者只跑一遍、不交回的。
    Done,
    /// 没过：把这段发回给同一个 CLI 修。
    Fix(String),
    /// 停下（修够了、同一个失败连续两次、两次之间没改文件），带原因。
    Stop(String),
    /// 站主按了停。
    Interrupted,
}

/// 准备一个 CLI 任务的验证要的东西。
pub struct Setup<'a> {
    pub checks: Vec<Check>,
    /// 目标模式（`*` 是全部）；空着是平时。
    pub goal: Vec<String>,
    pub cwd: &'a Path,
    pub level: PermissionLevel,
    pub env: Vec<(String, String)>,
    pub folders: &'a [(PathBuf, bool)],
    pub spill_dir: PathBuf,
    pub runtime_dir: PathBuf,
    /// 只跑一遍、不交回（Antigravity 只做直接调用；续接不了的 CLI）。
    pub once: bool,
}

pub struct CliVerify {
    monitor: Monitor,
    gate: FixGate,
    goal: Vec<String>,
    once: bool,
    sink: EventSink,
}

impl CliVerify {
    /// 在引擎开跑之前建：先记下项目文件的样子，做完才比得出改了哪些。PLAN、没有检查为空。
    pub async fn prepare(setup: Setup<'_>, sink: EventSink) -> Option<Self> {
        if setup.level == PermissionLevel::Plan || setup.checks.is_empty() {
            return None;
        }
        let ctx = ToolContext::new(setup.cwd, setup.level, setup.spill_dir, setup.runtime_dir, setup.env).ok()?.with_extra(setup.folders);
        let goal = if setup.goal.iter().any(|g| g == "*") { setup.checks.iter().map(|c| c.name.clone()).collect() } else { setup.goal };
        let monitor = Monitor::new(setup.checks, Arc::new(ctx), sink.clone(), permission::sandbox_for(setup.level)).await;
        Some(Self { monitor, gate: FixGate::default(), goal, once: setup.once, sink })
    }

    /// 目标模式：开头告诉 CLI 停下以后 Peck 会跑哪几条（平时不说，免得它不自己检查）。
    pub fn goal_note(&self) -> Option<String> {
        (!self.goal.is_empty()).then(|| {
            let list = self.goal.iter().filter_map(|n| self.monitor.checks().iter().find(|c| &c.name == n)).map(|c| format!("- {}: `{}`", c.name, c.run)).collect::<Vec<_>>().join("\n");
            format!(
                "\n\n<peck-goal>\nGoal mode: when you stop, Peck runs these checks in the project. If any fails, you get its output back and continue in this session until they all pass:\n{list}\n</peck-goal>"
            )
        })
    }

    /// 引擎这一轮做完了（做完，不是出错或停下）：比文件、跑要看的几条，决定收尾、交回去修还是停下。
    /// `stop` 变成 true 是站主按了停。
    pub async fn after_turn(&mut self, stop: &mut watch::Receiver<bool>) -> After {
        self.monitor.observe(&[]).await;
        let names = self.monitor.selection(&self.goal);
        if names.is_empty() {
            return After::Done;
        }
        let goal = !self.goal.is_empty();
        self.monitor.attempt = self.gate.fixes;
        let trigger = if goal { VerifyTrigger::Goal } else { VerifyTrigger::End };
        let results = {
            let fut = self.monitor.settle(&names, trigger);
            tokio::pin!(fut);
            tokio::select! {
                biased;
                r = &mut fut => Some(r),
                _ = stopped(stop) => None,
            }
        };
        let Some(results) = results else {
            self.monitor.stop();
            return After::Interrupted;
        };
        let max = if goal { GOAL_ATTEMPTS } else { FIX_ATTEMPTS };
        let failed: Vec<monitor::Outcome> = results.iter().filter(|o| !o.passed).cloned().collect();
        let round = |attempt, outcome, reason: Option<String>| EventBody::VerifyRound { attempt, max_attempts: max, outcome, reason, goal };
        if failed.is_empty() {
            self.sink.emit(round(self.gate.fixes, VerifyOutcome::Passed, None));
            return After::Done;
        }
        let names_failed = failed.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join("、");
        if self.once {
            self.sink.emit(round(self.gate.fixes, VerifyOutcome::Stopped, Some(format!("验证没过（{names_failed}）：这一家只跑一遍，Peck 不交回去修；失败的输出在验证卡片里。"))));
            return After::Done;
        }
        let attempt = self.gate.fixes;
        if let Err(reason) = self.gate.judge(&failed, self.monitor.total_version, max) {
            self.sink.emit(round(attempt, VerifyOutcome::Stopped, Some(reason.clone())));
            return After::Stop(format!("{reason}失败的输出在验证卡片里；接着说一句就能继续。"));
        }
        self.monitor.attempt = self.gate.fixes;
        self.sink.emit(round(self.gate.fixes, VerifyOutcome::Fixing, None));
        After::Fix(monitor::failure_message(&failed, self.gate.fixes, max, goal))
    }

    /// 任务结束或停下：还在跑的检查都停掉。
    pub fn stop(&mut self) {
        self.monitor.stop();
    }
}

/// 等到站主按停（已经按了就马上回）。
async fn stopped(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::tools::testing::TempProject;

    fn check(name: &str, run: &str) -> Check {
        Check { name: name.into(), run: run.into(), when: vec!["**/*.txt".into()], timeout: Some(30) }
    }

    async fn setup(p: &TempProject, goal: Vec<String>, once: bool) -> (CliVerify, std::sync::Arc<std::sync::Mutex<Vec<crate::gateway::event::AgentEvent>>>) {
        let (sink, events) = EventSink::memory("claude-code");
        let v = CliVerify::prepare(
            Setup {
                checks: vec![check("ok-file", "grep -q ok note.txt")],
                goal,
                cwd: &p.dir,
                level: PermissionLevel::Edit,
                env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
                folders: &[],
                spill_dir: p.dir.with_extension("spill"),
                runtime_dir: p.dir.with_extension("rt"),
                once,
            },
            sink,
        )
        .await
        .unwrap();
        (v, events)
    }

    fn rounds(events: &[crate::gateway::event::AgentEvent]) -> Vec<VerifyOutcome> {
        events.iter().filter_map(|e| match &e.body { EventBody::VerifyRound { outcome, .. } => Some(*outcome), _ => None }).collect()
    }

    #[tokio::test]
    async fn a_failing_check_goes_back_then_passes_and_nothing_changed_runs_nothing() {
        let p = TempProject::new();
        let (_tx, mut stop) = watch::channel(false);
        let (mut v, events) = setup(&p, vec![], false).await;
        // 没改文件：不跑。
        assert_eq!(v.after_turn(&mut stop).await, After::Done);
        std::fs::write(p.dir.join("note.txt"), "bad").unwrap();
        match v.after_turn(&mut stop).await {
            After::Fix(text) => assert!(text.contains("ok-file") && text.contains("auto-fix 1/3"), "{text}"),
            other => panic!("{other:?}"),
        }
        std::fs::write(p.dir.join("note.txt"), "ok").unwrap();
        assert_eq!(v.after_turn(&mut stop).await, After::Done);
        assert_eq!(rounds(&events.lock().unwrap()), vec![VerifyOutcome::Fixing, VerifyOutcome::Passed]);
        v.stop();
    }

    #[tokio::test]
    async fn the_same_failure_twice_stops_and_once_only_shows_the_result() {
        let p = TempProject::new();
        let (_tx, mut stop) = watch::channel(false);
        let (mut v, events) = setup(&p, vec![], false).await;
        std::fs::write(p.dir.join("note.txt"), "bad").unwrap();
        assert!(matches!(v.after_turn(&mut stop).await, After::Fix(_)));
        std::fs::write(p.dir.join("note.txt"), "still bad").unwrap();
        match v.after_turn(&mut stop).await {
            After::Stop(why) => assert!(why.contains("同一个失败连续两次"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(rounds(&events.lock().unwrap()), vec![VerifyOutcome::Fixing, VerifyOutcome::Stopped]);
        // Antigravity 这类：只跑一遍、写结果，不交回。
        let p = TempProject::new();
        let (mut v, events) = setup(&p, vec![], true).await;
        std::fs::write(p.dir.join("note.txt"), "bad").unwrap();
        assert_eq!(v.after_turn(&mut stop).await, After::Done);
        assert_eq!(rounds(&events.lock().unwrap()), vec![VerifyOutcome::Stopped]);
    }

    #[tokio::test]
    async fn goal_mode_runs_without_edits_and_tells_the_cli() {
        let p = TempProject::new();
        let (_tx, mut stop) = watch::channel(false);
        let (mut v, _) = setup(&p, vec!["*".into()], false).await;
        assert!(v.goal_note().unwrap().contains("grep -q ok note.txt"));
        match v.after_turn(&mut stop).await {
            After::Fix(text) => assert!(text.contains("auto-fix 1/8"), "{text}"),
            other => panic!("{other:?}"),
        }
        let (tx, mut stop) = watch::channel(false);
        tx.send(true).unwrap();
        std::fs::write(p.dir.join("note.txt"), "x").unwrap();
        assert_eq!(v.after_turn(&mut stop).await, After::Interrupted);
    }
}
