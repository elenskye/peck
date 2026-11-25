//! CLI 引擎的脚本 Hook（v1.1.4）：Peck 替 CLI 跑它控制得了的三个时刻，格式和 Peck Loop 的一样（`harness/script_hooks.rs`）。
//!
//! - **开场时**（`SessionStart`，source `startup`）：新会话（不是续上的）第一句发出去之前；**发话时**（`UserPromptSubmit`）：
//!   每句发出去之前。stdout 当补充上下文放在这句前面；发话时退出码 2 就不发这句话，开场时拦不住、只记一笔。
//! - **收尾时**（`Stop`）：CLI 做完了（失败、停下的不跑）再跑；退出码 2 就续上它的会话、把原因发过去让它接着干，一回合最多
//!   3 次。排在 Git Plan 的收尾提交之前。续不了会话的写一行，照常收尾。
//! - 调工具前后、压缩前是 CLI 自己的事，Peck 管不到，不跑。
//! - **从哪读**：和 Peck Loop 一样。Claude Code 自己会跑项目 `.claude/settings.json` 里的，Grok 在它信任过的文件夹里也会
//!   （它的 Claude 兼容，v1.1.9 验收时发现跑了两遍），这两种 Peck 只替它跑插件带的。
//! - **卡片**：每条命令第一次跑之前弹卡片，「总是允许」和 Peck Loop 共用（`hook:<指纹>`）；拒了这一回合不再问、不跑。
//!   请求号 `hook-approve-<n>`：控制通道里这些归 Peck（[`Router`]），别的转给正在跑的那一次引擎。

use super::event::{ApprovalDecision, EventBody, PermissionLevel, ResolvedBy, ToolKind};
use super::log::EventSink;
use super::Control;
use crate::harness::script_hooks::{self, Event, HookSet};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Peck 自己弹的 Hook 卡片的请求号前缀。
const PREFIX: &str = "hook-approve-";

/// 这一条是不是 Peck 自己弹的 Hook 卡片（引擎的审批规则不碰它）。
pub fn is_hook_card(body: &EventBody) -> bool {
    matches!(body, EventBody::ApprovalRequested { request_id, .. } if request_id.starts_with(PREFIX))
}

/// 这个引擎这次要 Peck 替它跑哪些（没有就是 `None`，照旧直接把控制通道交给引擎）。`cli_runs_project`：CLI 自己会跑
/// 项目 `.claude/settings.json` 里的（Claude Code 总是；Grok 信任了这个文件夹时），那就只跑插件带的。
pub fn hooks_for(engine: &str, cwd: &Path, cli_runs_project: bool) -> Option<HookSet> {
    let set = HookSet::load(cwd);
    let set = if engine == "claude-code" || cli_runs_project { set.only_plugins() } else { set };
    let wanted = [Event::SessionStart, Event::UserPromptSubmit, Event::Stop];
    (wanted.iter().any(|e| !set.for_event(*e, None).is_empty())).then_some(set)
}

/// 一个 CLI 任务的控制通道（v1.1.4）：界面发来的都先到这里。Peck 的 Hook 卡片的回答留给 Peck；停下记一笔、也转给引擎；
/// 别的转给正在跑的那一次引擎（收尾 Hook 让它接着干时，每一次是新的一条通道）。引擎没在跑时追加的话先攒着。
struct Router {
    current: Arc<Mutex<Option<mpsc::UnboundedSender<Control>>>>,
    pending: Arc<Mutex<Vec<Control>>>,
    interrupted: Arc<AtomicBool>,
    hook_rx: mpsc::UnboundedReceiver<Control>,
}

impl Router {
    fn start(mut rx: mpsc::UnboundedReceiver<Control>) -> Self {
        let (hook_tx, hook_rx) = mpsc::unbounded_channel();
        let current: Arc<Mutex<Option<mpsc::UnboundedSender<Control>>>> = Arc::default();
        let pending: Arc<Mutex<Vec<Control>>> = Arc::default();
        let interrupted = Arc::new(AtomicBool::new(false));
        let (to, waiting, stopped) = (current.clone(), pending.clone(), interrupted.clone());
        tokio::spawn(async move {
            while let Some(c) = rx.recv().await {
                match &c {
                    Control::Approve { request_id, .. } if request_id.starts_with(PREFIX) => {
                        let _ = hook_tx.send(c);
                    }
                    // Hook 卡片只认站主点的；规则批的不算（也不该有）。
                    Control::ApproveByRule { request_id } if request_id.starts_with(PREFIX) => {}
                    Control::Interrupt => {
                        stopped.store(true, Ordering::SeqCst);
                        let _ = hook_tx.send(Control::Interrupt);
                        if let Some(tx) = to.lock().ok().and_then(|t| t.clone()) {
                            let _ = tx.send(Control::Interrupt);
                        }
                    }
                    _ => match to.lock().ok().and_then(|t| t.clone()) {
                        Some(tx) => {
                            let _ = tx.send(c);
                        }
                        None if matches!(c, Control::Append { .. }) => {
                            if let Ok(mut p) = waiting.lock() {
                                p.push(c);
                            }
                        }
                        None => {}
                    },
                }
            }
        });
        Self { current, pending, interrupted, hook_rx }
    }

    /// 给下一次引擎一条自己的通道：攒着的追加先放进去；已经按过停就先放一个打断。
    fn engine_channel(&self) -> mpsc::UnboundedReceiver<Control> {
        let (tx, rx) = mpsc::unbounded_channel();
        if let Ok(mut p) = self.pending.lock() {
            for c in p.drain(..) {
                let _ = tx.send(c);
            }
        }
        if let Ok(mut cur) = self.current.lock() {
            *cur = Some(tx.clone());
        }
        if self.interrupted.load(Ordering::SeqCst) {
            let _ = tx.send(Control::Interrupt);
        }
        rx
    }

    fn engine_done(&self) {
        if let Ok(mut cur) = self.current.lock() {
            *cur = None;
        }
    }
}

/// 一个 CLI 任务的脚本 Hook。
pub struct CliHooks {
    set: HookSet,
    sink: EventSink,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    level: PermissionLevel,
    session: String,
    allowed: HashSet<String>,
    remember: Arc<dyn Fn(&str) + Send + Sync>,
    /// 这一回合批没批（规则名 → 批了）。
    ok: HashMap<String, bool>,
    approvals: u32,
    router: Router,
    /// 收尾时拦了几次。
    pub stop_blocks: u32,
}

impl CliHooks {
    /// `control` 是这个任务的控制通道（交给 [`Router`]，引擎每次从 [`Self::engine_channel`] 拿自己的）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        set: HookSet,
        control: mpsc::UnboundedReceiver<Control>,
        sink: EventSink,
        cwd: &Path,
        env: Vec<(String, String)>,
        level: PermissionLevel,
        session: String,
        allowed: HashSet<String>,
        remember: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        Self { set, sink, cwd: cwd.to_path_buf(), env, level, session, allowed, remember, ok: HashMap::new(), approvals: 0, router: Router::start(control), stop_blocks: 0 }
    }

    pub fn has(&self, event: Event) -> bool {
        !self.set.for_event(event, None).is_empty()
    }

    /// 站主按过停（卡片等着时也算）。
    pub fn interrupted(&self) -> bool {
        self.router.interrupted.load(Ordering::SeqCst)
    }

    pub fn engine_channel(&self) -> mpsc::UnboundedReceiver<Control> {
        self.router.engine_channel()
    }

    pub fn engine_done(&self) {
        self.router.engine_done();
    }

    /// 跑这个时刻挂的。返回拦下的原因、补充的话（几条的合在一起）。`target` 是 matcher 要对的（开场时是 source）。
    pub async fn run(&mut self, event: Event, target: Option<&str>, extra: Value) -> (Option<String>, Option<String>) {
        let hooks = self.set.for_event(event, target);
        if hooks.is_empty() {
            return (None, None);
        }
        let input = script_hooks::input(event, &self.session, &self.cwd, self.level, extra);
        let (mut blocks, mut contexts) = (Vec::new(), Vec::new());
        for hook in hooks {
            if self.interrupted() {
                break;
            }
            let rule = hook.rule();
            let ok = match self.ok.get(&rule).copied() {
                Some(ok) => ok,
                None if hook.trusted || self.allowed.contains(&rule) => true,
                None => {
                    let decision = self.approve(&hook).await;
                    if decision == ApprovalDecision::AllowAlways {
                        self.allowed.insert(rule.clone());
                        (self.remember)(&rule);
                    }
                    decision.allows()
                }
            };
            self.ok.insert(rule, ok);
            if !ok || self.interrupted() {
                continue;
            }
            let (v, note) = script_hooks::run_and_judge(&hook, &input, &self.cwd, &self.env).await;
            if let Some(body) = note {
                self.sink.emit(body);
            }
            blocks.extend(v.block);
            contexts.extend(v.context);
        }
        let join = |v: Vec<String>| (!v.is_empty()).then(|| v.join("\n\n"));
        (join(blocks), join(contexts))
    }

    /// 弹一张卡片等站主批（和 Peck Loop 的一样：批准一次、总是允许、不行）。按了停就当不行。
    async fn approve(&mut self, hook: &script_hooks::Hook) -> ApprovalDecision {
        self.approvals += 1;
        let request_id = format!("{PREFIX}{}", self.approvals);
        let options = vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce];
        self.sink.emit(EventBody::ApprovalRequested {
            reason: None,
            request_id: request_id.clone(),
            call_id: None,
            kind: ToolKind::Run,
            title: format!("跑项目的 Hook：{}", hook.title()),
            detail: Some(format!("来自 {}\n{}", hook.source, hook.command)),
            high_risk: false,
            options: options.clone(),
        });
        let decision = loop {
            match self.router.hook_rx.recv().await {
                Some(Control::Approve { request_id: id, decision }) if id == request_id => {
                    break if options.contains(&decision) { decision } else if decision.allows() { ApprovalDecision::AllowOnce } else { ApprovalDecision::DenyOnce };
                }
                Some(Control::Interrupt) | None => break ApprovalDecision::DenyOnce,
                Some(_) => continue,
            }
        };
        if !self.interrupted() {
            self.sink.emit(EventBody::ApprovalResolved { request_id, decision, by: ResolvedBy::User });
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::AgentEvent;

    fn project(settings: &str, plugin_hooks: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("peck-cli-hooks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), settings).unwrap();
        if let Some(h) = plugin_hooks {
            let p = dir.join(".claude/plugins/demo");
            std::fs::create_dir_all(p.join(".claude-plugin")).unwrap();
            std::fs::create_dir_all(p.join("hooks")).unwrap();
            std::fs::write(p.join(".claude-plugin/plugin.json"), r#"{"name":"demo","version":"1.0.0","description":"d"}"#).unwrap();
            std::fs::write(p.join("hooks/hooks.json"), h).unwrap();
        }
        dir
    }

    #[test]
    fn claude_code_only_gets_plugin_hooks() {
        let dir = project(r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo settings"}]}]}}"#, None);
        assert!(hooks_for("codex", &dir, false).is_some(), "Codex 不读 .claude/settings.json，Peck 替它跑");
        assert!(hooks_for("claude-code", &dir, false).is_none(), "Claude Code 自己跑 settings 里的");
        assert!(hooks_for("grok", &dir, true).is_none(), "信任了文件夹的 Grok 也自己跑");
        // 只挂了调工具前后的：CLI 这边一条都不用跑。
        let tools_only = project(r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"x"}]}]}}"#, None);
        assert!(hooks_for("codex", &tools_only, false).is_none());
        let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(tools_only));
    }

    #[tokio::test]
    async fn cards_go_to_peck_and_the_rest_to_the_engine() {
        let (sink, events) = EventSink::memory("codex");
        let dir = std::env::temp_dir();
        let set = HookSet { hooks: script_hooks::parse(&serde_json::json!({"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "echo 今天周二"}]}]}}), ".claude/settings.json", None) };
        let (tx, rx) = mpsc::unbounded_channel();
        let remembered: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = remembered.clone();
        let env = vec![("PATH".to_string(), std::env::var("PATH").unwrap_or_default())];
        let mut hooks = CliHooks::new(set, rx, sink, &dir, env, PermissionLevel::Edit, "s".into(), HashSet::new(), Arc::new(move |r: &str| seen.lock().unwrap().push(r.into())));
        // 卡片还没弹时追加的一句：攒着，交给下一次引擎。
        tx.send(Control::Append { text: "顺便".into() }).unwrap();
        let answer = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            answer.send(Control::Approve { request_id: "hook-approve-1".into(), decision: ApprovalDecision::AllowAlways }).unwrap();
        });
        let (block, context) = hooks.run(Event::UserPromptSubmit, None, serde_json::json!({"prompt": "改一下"})).await;
        assert_eq!((block, context.as_deref()), (None, Some("今天周二")));
        assert_eq!(remembered.lock().unwrap().len(), 1, "总是允许记下了");
        let got: Vec<AgentEvent> = events.lock().unwrap().clone();
        assert!(got.iter().any(|e| matches!(&e.body, EventBody::ApprovalRequested { request_id, call_id: None, .. } if request_id == "hook-approve-1")));
        let mut engine = hooks.engine_channel();
        assert!(matches!(engine.recv().await, Some(Control::Append { .. })), "攒着的追加先到");
        tx.send(Control::ApproveByRule { request_id: "codex-1".into() }).unwrap();
        assert!(matches!(engine.recv().await, Some(Control::ApproveByRule { .. })), "引擎自己的审批转给它");
        tx.send(Control::Interrupt).unwrap();
        assert!(matches!(engine.recv().await, Some(Control::Interrupt)));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(hooks.interrupted());
        hooks.engine_done();
        // 第二次同一条命令不再问（这一回合批过）。
        let (_, context) = hooks.run(Event::UserPromptSubmit, None, serde_json::json!({})).await;
        assert_eq!(context, None, "按过停就不再跑");
    }
}
