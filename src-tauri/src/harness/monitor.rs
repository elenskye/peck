//! 替模型盯验证命令（v0.6 Phase 8）。Peck 循环里一个任务一个 `Monitor`：
//!
//! - **改了文件就在后台跑**：每轮工具跑完，比一遍项目文件（大小 + 修改时间，照 `.gitignore`），改到了哪条验证命令
//!   管的文件（`when`），就在后台起它；模型接着干活，不等。
//! - **并发过期**：一条验证命令跑的时候它管的文件又被改了，这次的结果不算（`stale`），停掉、按新的文件再跑。
//! - **模型停下时收尾**（`settle`）：要看的那几条（目标模式是目标那几条；平时是这个任务改过的文件管得到的），
//!   结果是新的就直接用，正在按最新文件跑的就等它，没跑过的现在跑。都过了就收尾、不再叫模型；没过由循环决定
//!   交回去修还是停下（`loop.rs`：最多修 3 次；同一个失败连续两次、两次之间没改文件都停）。

use super::spill;
use super::tools::bash::{self, Exec, Sandbox};
use super::tools::{tail_bytes, ToolContext};
use super::verify::Check;
use crate::gateway::event::{EventBody, VerifyStatus, VerifyTrigger};
use crate::gateway::log::EventSink;
use std::collections::{BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// 项目文件太多就不比了（只认工具自己报的改动）。
const MAX_FILES: usize = 50_000;
/// 交给模型的失败输出留结尾这么多。
pub const FOR_MODEL: usize = 3 * 1024;
/// 事件里带的输出留结尾这么多。
const FOR_VIEW: usize = 4 * 1024;

/// 一条验证命令跑完的样子。
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub name: String,
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// 输出（太长的已落盘，这里是开头结尾 + 路径）。
    pub output: String,
    pub duration_ms: u64,
    /// 跑的时候这条命令管的文件是第几版。
    pub version: u64,
    /// 失败的样子（认「同一个失败」用）。
    pub signature: u64,
}

struct Running {
    check_id: String,
    version: u64,
    cancel: watch::Sender<bool>,
    handle: JoinHandle<Option<Outcome>>,
}

type Fingerprint = HashMap<String, (u64, u128)>;

pub struct Monitor {
    checks: Vec<Check>,
    ctx: Arc<ToolContext>,
    sink: EventSink,
    sandbox: Sandbox,
    /// 每条命令管的文件改过几次。
    versions: HashMap<String, u64>,
    /// 项目里的文件一共变过几次（「两次之间没改文件」用）。
    pub total_version: u64,
    running: HashMap<String, Running>,
    latest: HashMap<String, Outcome>,
    counter: u32,
    fingerprint: Option<Fingerprint>,
    /// 比不了文件时（项目太大）：工具报的每个文件的 Diff 上次是什么样（只有变了的才算改了）。
    reported: HashMap<String, u64>,
    /// 这个任务里改过的文件。
    pub changed: BTreeSet<String>,
    /// 现在是第几次自动修之后（事件里写）。
    pub attempt: u32,
}

impl Monitor {
    pub async fn new(checks: Vec<Check>, ctx: Arc<ToolContext>, sink: EventSink, sandbox: Sandbox) -> Self {
        let root = ctx.root.clone();
        let fingerprint = tokio::task::spawn_blocking(move || fingerprint(&root)).await.ok().flatten();
        Self {
            versions: checks.iter().map(|c| (c.name.clone(), 0)).collect(),
            checks,
            ctx,
            sink,
            sandbox,
            total_version: 0,
            running: HashMap::new(),
            latest: HashMap::new(),
            counter: 0,
            fingerprint,
            reported: HashMap::new(),
            changed: BTreeSet::new(),
            attempt: 0,
        }
    }

    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// 一轮工具跑完：看改了哪些文件；改到的验证命令在后台（重新）起。`reported` 是工具自己报的改动
    /// （这个任务到现在的 Diff，比不了文件时用：只算 Diff 和上一轮不一样的）。
    pub async fn observe(&mut self, reported: &[crate::gateway::event::FileDiff]) -> Vec<String> {
        let root = self.ctx.root.clone();
        let now = tokio::task::spawn_blocking(move || fingerprint(&root)).await.ok().flatten();
        let mut changed: Vec<String> = match (&self.fingerprint, &now) {
            (Some(old), Some(new)) => diff(old, new),
            _ => {
                let mut out = Vec::new();
                for d in reported {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    d.patch.hash(&mut h);
                    let sig = h.finish();
                    if self.reported.insert(d.path.clone(), sig) != Some(sig) {
                        out.push(d.path.clone());
                    }
                }
                out
            }
        };
        if now.is_some() {
            self.fingerprint = now;
        }
        changed.sort();
        changed.dedup();
        if changed.is_empty() {
            return changed;
        }
        self.total_version += 1;
        self.changed.extend(changed.iter().cloned());
        let hit: Vec<Check> = self.checks.iter().filter(|c| c.matches(&changed)).cloned().collect();
        for check in hit {
            *self.versions.entry(check.name.clone()).or_default() += 1;
            self.supersede(&check.name);
            self.spawn(&check, VerifyTrigger::Edit);
        }
        changed
    }

    /// 正在跑的这条作废（文件又变了）：停掉，记一条「过期」。
    fn supersede(&mut self, name: &str) {
        if let Some(r) = self.running.remove(name) {
            if r.handle.is_finished() {
                // 刚好跑完：结果按它当时的版本收下（已经比现在旧，收尾时会再跑）。
                if let Some(Some(o)) = r.handle.now_or_never_result() {
                    self.latest.insert(name.to_string(), o);
                }
                return;
            }
            let _ = r.cancel.send(true);
            r.handle.abort();
            self.sink.emit(EventBody::VerifyCompleted {
                check_id: r.check_id,
                name: name.to_string(),
                status: VerifyStatus::Stale,
                exit_code: None,
                duration_ms: None,
                output: Some("跑的时候文件又改了，这次的结果不算，按新的文件再跑。".into()),
            });
        }
    }

    fn spawn(&mut self, check: &Check, trigger: VerifyTrigger) {
        self.counter += 1;
        let check_id = format!("v{}", self.counter);
        let version = self.versions.get(&check.name).copied().unwrap_or(0);
        self.sink.emit(EventBody::VerifyStarted {
            check_id: check_id.clone(),
            name: check.name.clone(),
            command: check.run.clone(),
            trigger,
            attempt: self.attempt,
        });
        let (cancel, cancel_rx) = watch::channel(false);
        let ctx = self.ctx.clone();
        let sink = self.sink.clone();
        let name = check.name.clone();
        let check = check.clone();
        let sandbox = self.sandbox;
        let id = check_id.clone();
        let handle = tokio::spawn(async move {
            let o = run_check(&ctx, &check, sandbox, version, cancel_rx).await?;
            sink.emit(EventBody::VerifyCompleted {
                check_id: id,
                name: o.name.clone(),
                status: if o.passed { VerifyStatus::Passed } else { VerifyStatus::Failed },
                exit_code: o.exit_code,
                duration_ms: Some(o.duration_ms),
                output: Some(tail_bytes(&o.output, FOR_VIEW).to_string()),
            });
            Some(o)
        });
        self.running.insert(name, Running { check_id, version, cancel, handle });
    }

    /// 收尾时要看哪几条：目标模式是目标那几条；平时是这个任务改过的文件管得到的。
    pub fn selection(&self, goal: &[String]) -> Vec<String> {
        if !goal.is_empty() {
            return self.checks.iter().filter(|c| goal.iter().any(|g| g == "*" || g == &c.name)).map(|c| c.name.clone()).collect();
        }
        let changed: Vec<String> = self.changed.iter().cloned().collect();
        self.checks.iter().filter(|c| c.matches(&changed)).map(|c| c.name.clone()).collect()
    }

    /// 模型停下了：这几条按现在的文件的结果（新的直接用，在跑的等，没跑的现在跑）。
    pub async fn settle(&mut self, names: &[String], trigger: VerifyTrigger) -> Vec<Outcome> {
        let mut waits: Vec<String> = Vec::new();
        let mut done: Vec<Outcome> = Vec::new();
        for name in names {
            let Some(check) = self.checks.iter().find(|c| &c.name == name).cloned() else { continue };
            let version = self.versions.get(name).copied().unwrap_or(0);
            if let Some(o) = self.latest.get(name).filter(|o| o.version == version) {
                done.push(o.clone());
                continue;
            }
            let fresh = self.running.get(name).is_some_and(|r| r.version == version);
            if !fresh {
                self.supersede(name);
                self.spawn(&check, trigger);
            }
            if self.running.contains_key(name) {
                waits.push(name.clone());
            }
        }
        // 等的时候被打断（这个 future 被丢掉）：句柄还在 `running` 里，`stop` 停得掉。
        for name in waits {
            let Some(r) = self.running.get_mut(&name) else { continue };
            let result = (&mut r.handle).await;
            self.running.remove(&name);
            if let Ok(Some(o)) = result {
                self.latest.insert(name, o.clone());
                done.push(o);
            }
        }
        // 按配置里的顺序
        done.sort_by_key(|o| self.checks.iter().position(|c| c.name == o.name).unwrap_or(usize::MAX));
        done
    }

    /// 后台跑完了的收下（不等）。
    pub fn collect(&mut self) {
        let finished: Vec<String> = self.running.iter().filter(|(_, r)| r.handle.is_finished()).map(|(n, _)| n.clone()).collect();
        for name in finished {
            if let Some(r) = self.running.remove(&name) {
                if let Some(Some(o)) = r.handle.now_or_never_result() {
                    self.latest.insert(name, o);
                }
            }
        }
    }

    /// 任务结束或被打断：还在跑的都停掉（已经跑完、报过结果的先收下，不再记成「停了」）。
    pub fn stop(&mut self) {
        self.collect();
        for (name, r) in self.running.drain() {
            let _ = r.cancel.send(true);
            r.handle.abort();
            self.sink.emit(EventBody::VerifyCompleted {
                check_id: r.check_id,
                name,
                status: VerifyStatus::Cancelled,
                exit_code: None,
                duration_ms: None,
                output: None,
            });
        }
    }
}

/// `JoinHandle` 已经跑完时取结果（不阻塞）。
trait NowOrNever<T> {
    fn now_or_never_result(self) -> Option<T>;
}

impl<T: Send + 'static> NowOrNever<T> for JoinHandle<T> {
    fn now_or_never_result(self) -> Option<T> {
        if !self.is_finished() {
            self.abort();
            return None;
        }
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut handle = self;
        match std::pin::Pin::new(&mut handle).poll(&mut cx) {
            std::task::Poll::Ready(Ok(v)) => Some(v),
            _ => None,
        }
    }
}

use std::future::Future;

async fn run_check(ctx: &ToolContext, check: &Check, sandbox: Sandbox, version: u64, cancel: watch::Receiver<bool>) -> Option<Outcome> {
    let started = Instant::now();
    let result = bash::run_command(
        ctx,
        Exec {
            command: &check.run,
            cwd: &ctx.root,
            timeout: Duration::from_secs(check.timeout_secs()),
            sandbox,
            keep_bytes: 8 * 1024 * 1024,
            stream_bytes: 0,
        },
        cancel,
        |_| {},
    )
    .await;
    let (passed, exit_code, timed_out, text) = match result {
        Ok(o) if o.cancelled => return None,
        Ok(o) => {
            let mut text = o.status_line();
            if !o.output.trim().is_empty() {
                text.push('\n');
                text.push_str(&o.output);
            }
            (o.exit_code == Some(0), o.exit_code, o.timed_out, text)
        }
        Err(e) => (false, None, false, e),
    };
    let label = format!("verify-{}", sanitize(&check.name));
    let (output, _) = spill::shape(ctx, &label, &text, !passed);
    Some(Outcome {
        name: check.name.clone(),
        command: check.run.clone(),
        passed,
        exit_code,
        timed_out,
        signature: signature(&check.name, exit_code, &text),
        output,
        duration_ms: started.elapsed().as_millis() as u64,
        version,
    })
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// 「同一个失败」：命令名、退出码，加上输出里像错误的那几行（去掉耗时、地址这类每次都变的）。
pub fn signature(name: &str, exit_code: Option<i32>, output: &str) -> u64 {
    let markers = ["error", "fail", "panic", "assert", "✗", "×", "expected", "cannot", "undefined", "not found"];
    let mut lines: Vec<String> = output
        .lines()
        .filter(|l| {
            let low = l.to_lowercase();
            markers.iter().any(|m| low.contains(m))
        })
        .take(60)
        .map(normalize)
        .collect();
    if lines.is_empty() {
        let all: Vec<&str> = output.lines().collect();
        let skip = all.len().saturating_sub(30);
        lines = all[skip..].iter().map(|l| normalize(l).chars().filter(|c| !c.is_ascii_digit()).collect()).collect();
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    exit_code.hash(&mut h);
    lines.hash(&mut h);
    h.finish()
}

/// 去掉每次都变的：耗时（`12ms`、`3.4s`、`(0.52s)`）、十六进制地址、空白。
fn normalize(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let words: Vec<&str> = line.split_whitespace().collect();
    for w in words {
        let bare = w.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | ','));
        let is_time = {
            let num = bare.trim_end_matches("ms").trim_end_matches('s').trim_end_matches('m');
            num != bare && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit() || c == '.')
        };
        let is_addr = bare.starts_with("0x") && bare.len() > 4;
        if is_time || is_addr {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out.to_lowercase()
}

/// 项目里每个文件（照 `.gitignore`，不进 `.git`）的大小和修改时间。太多了为空。
fn fingerprint(root: &Path) -> Option<Fingerprint> {
    let mut out = HashMap::new();
    let walker = ignore::WalkBuilder::new(root).hidden(false).require_git(false).git_ignore(true).parents(false).build();
    for entry in walker.flatten() {
        let path = entry.path();
        if path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        out.insert(rel, (meta.len(), mtime));
        if out.len() > MAX_FILES {
            return None;
        }
    }
    Some(out)
}

fn diff(old: &Fingerprint, new: &Fingerprint) -> Vec<String> {
    let mut changed: Vec<String> = new.iter().filter(|(k, v)| old.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect();
    changed.extend(old.keys().filter(|k| !new.contains_key(*k)).cloned());
    changed
}

/// 交给模型修的那一条（`<peck-verify>`）。
pub fn failure_message(failed: &[Outcome], attempt: u32, max: u32, goal: bool) -> String {
    let mut text = format!(
        "{}\nPeck ran the project's verify commands after you stopped. {} (auto-fix {attempt}/{max}):\n",
        super::compact::VERIFY_TAG,
        if goal { "The goal is not met yet" } else { "Not passing" }
    );
    for o in failed {
        let head = if o.timed_out {
            "timed out".to_string()
        } else {
            o.exit_code.map(|c| format!("exit code {c}")).unwrap_or_else(|| "did not finish".into())
        };
        text.push_str(&format!(
            "\n## {}: `{}` — {head} ({:.1}s)\n```\n{}\n```\n",
            o.name,
            o.command,
            o.duration_ms as f64 / 1000.0,
            tail_bytes(&o.output, FOR_MODEL).trim()
        ));
    }
    text.push_str("\nFix the cause, then stop: Peck re-runs these checks for you. Read the full output file if the tail is not enough.\n</peck-verify>");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::{AgentEvent, PermissionLevel};
    use crate::harness::tools::testing::TempProject;

    fn check(name: &str, run: &str, when: &[&str]) -> Check {
        Check { name: name.into(), run: run.into(), when: when.iter().map(|s| s.to_string()).collect(), timeout: Some(30) }
    }

    fn statuses(events: &[AgentEvent]) -> Vec<(String, VerifyStatus)> {
        events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::VerifyCompleted { name, status, .. } => Some((name.clone(), *status)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn signatures_ignore_timings_but_not_the_error() {
        let a = signature("t", Some(1), "running 3 tests\nerror[E0308]: mismatched types src/a.rs:3:5\ntest result: FAILED. 2 passed (0.52s)");
        let b = signature("t", Some(1), "running 3 tests\nerror[E0308]: mismatched types src/a.rs:3:5\ntest result: FAILED. 2 passed (1.07s)");
        let c = signature("t", Some(1), "running 3 tests\nerror[E0425]: cannot find value src/a.rs:9:1\ntest result: FAILED. 2 passed (0.52s)");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, signature("u", Some(1), "error[E0308]: mismatched types src/a.rs:3:5"));
    }

    #[tokio::test]
    async fn edits_start_matching_checks_and_newer_edits_make_them_stale() {
        let p = TempProject::new();
        p.write("src/a.rs", "1").write("README.md", "x");
        let ctx = Arc::new(p.ctx(PermissionLevel::Edit));
        let (sink, events) = EventSink::memory("peck");
        let checks = vec![
            check("slow", "sleep 2; test -f src/a.rs", &["**/*.rs"]),
            check("docs", "grep -q x README.md", &["*.md"]),
        ];
        let mut m = Monitor::new(checks, ctx, sink, Sandbox::Project).await;
        assert!(m.observe(&[]).await.is_empty(), "没改就不跑");
        p.write("src/a.rs", "22");
        assert_eq!(m.observe(&[]).await, vec!["src/a.rs".to_string()]);
        // 又改了一次：上一次的作废
        p.write("src/a.rs", "333");
        m.observe(&[]).await;
        assert_eq!(statuses(&events.lock().unwrap()), vec![("slow".to_string(), VerifyStatus::Stale)]);
        let results = m.settle(&m.selection(&[]), VerifyTrigger::End).await;
        assert_eq!(results.len(), 1, "README 没改，docs 不用看");
        assert!(results[0].passed && results[0].version == 2);
        // 结果是新的：再收尾一次不重跑
        let started = events.lock().unwrap().iter().filter(|e| matches!(e.body, EventBody::VerifyStarted { .. })).count();
        let again = m.settle(&["slow".to_string()], VerifyTrigger::End).await;
        assert_eq!(again, results);
        assert_eq!(events.lock().unwrap().iter().filter(|e| matches!(e.body, EventBody::VerifyStarted { .. })).count(), started);
        // 目标模式：没改也跑目标那几条
        let goal = m.settle(&m.selection(&["docs".to_string()]), VerifyTrigger::Goal).await;
        assert_eq!(goal.len(), 1);
        assert_eq!(goal[0].name, "docs");
        assert!(m.total_version == 2 && m.changed.len() == 1);
    }

    #[tokio::test]
    async fn failures_carry_the_output_and_stop_cancels() {
        let p = TempProject::new();
        p.write("a.txt", "1");
        let ctx = Arc::new(p.ctx(PermissionLevel::Edit));
        let (sink, events) = EventSink::memory("peck");
        let mut m = Monitor::new(vec![check("t", "echo 'error: boom' >&2; exit 2", &[]), check("hang", "sleep 30", &[])], ctx, sink, Sandbox::Project).await;
        p.write("a.txt", "2");
        m.observe(&[]).await;
        let r = m.settle(&["t".to_string()], VerifyTrigger::End).await;
        assert!(!r[0].passed && r[0].exit_code == Some(2) && r[0].output.contains("error: boom"));
        let msg = failure_message(&r, 1, 3, false);
        assert!(msg.starts_with("<peck-verify>") && msg.contains("exit code 2") && msg.contains("1/3"));
        m.stop();
        assert!(statuses(&events.lock().unwrap()).contains(&("hang".to_string(), VerifyStatus::Cancelled)));
    }
}
