//! CLI 也能委派（v0.9 Phase 14 B1）：CLI 引擎（Claude Code、Codex、Grok；Antigravity 只做直接调用，不委派；Muse 还没接上）
//! 跑任务时，Peck 的本机 MCP（`gateway/mcp.rs`）多给一个 `delegate_task`。CLI 调它，Peck 用 AGENTS 里选的委派模型
//! 起一个子任务：Peck Loop，同一个项目（独立工作区里就是那个工作区）、同一档权限，不能再委派、不问站主、不写清单
//! 和记忆，最多 24 轮。子任务的事件进这个任务的事件日志（带 `subagentId`，界面挂在 CLI 那一步下面），审批照样弹给
//! 站主（编号带前缀，`runner::route_control` 按前缀转过来），用量带模型 id、算进任务和花费上限。跑完把报告和
//! 改了的文件回给 CLI。一个任务（会话）最多 6 次，前几句用掉的也算。

use super::context::ProjectLayer;
use super::r#loop::{ContextConfig, Delegate, LoopConfig, PeckBackend, MAX_DELEGATIONS};
use super::run::{budget, live_spend, memories_of, model_label, runtime_dir, spill_root, with_project_env};
use crate::gateway::event::{AgentEvent, EventBody, PermissionLevel, ToolStatus, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{runner, AgentBackend, AgentTask};
use crate::store::Store;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const SUB_BRIEF: &str = "You are a sub-agent: the main agent handed you one subtask (below). Do exactly that, then report briefly what you did, what you changed and anything you could not do. Don't ask the user questions and don't widen the scope.";

/// 一个 CLI 任务能用的委派。
pub struct CliDelegate {
    pub delegate: Delegate,
    pub label: String,
    run_id: String,
    cwd: PathBuf,
    level: PermissionLevel,
    config: LoopConfig,
    store: Arc<Mutex<Store>>,
    workspace_id: String,
    /// 这个任务（会话）里已经委派了几次（前几句的也算）。
    used: AtomicU32,
    /// 这一句里的第几个子任务（编号用）。
    count: AtomicU32,
}

/// 给 CLI 的这一句准备委派。`cwd` 是 CLI 干活的目录（独立工作区就是工作区）；其他文件夹给子任务只读。
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    store: &Arc<Mutex<Store>>,
    delegate: Delegate,
    run_id: &str,
    session_id: &str,
    cwd: &Path,
    level: PermissionLevel,
    cost_cap: Option<f64>,
) -> Result<Arc<CliDelegate>, String> {
    let (ws, used, allowed) = {
        let db = store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace_for_session(session_id).map_err(|e| e.to_string())?;
        let used = db.delegations_in_session(session_id).unwrap_or(0);
        let allowed = db.harness_allowed(&ws.id).unwrap_or_default();
        (ws, used, allowed)
    };
    let workspace = PathBuf::from(&ws.path);
    let worktree_of = (cwd != workspace.as_path()).then(|| workspace.clone());
    let config = LoopConfig {
        model_id: delegate.model_id.clone(),
        max_rounds: delegate.rounds,
        budget: budget(store, cost_cap),
        live_spend: Some(live_spend(store)),
        oneshot: false,
        allowed,
        spill_dir: spill_root().join(session_id),
        runtime_dir: runtime_dir(&workspace),
        env: with_project_env(super::tools::bash::shell_env(), &ws.settings.env_pairs()),
        session_ref: None,
        context: ContextConfig {
            project: ProjectLayer::load(&workspace, memories_of(store, &ws.id)).with_skills(&workspace, &ws.settings.skills_off),
            environment: true,
            folders: ws.settings.live_folders().into_iter().map(|(p, _)| (p, false)).collect(),
            env_names: ws.settings.env.iter().map(|v| v.name.clone()).collect(),
            worktree_of,
            ..Default::default()
        },
        delegate: None,
        brief: Some(SUB_BRIEF.into()),
        extras: None,
        task_level: None,
        mcp: vec![],
        mcp_slot: None,
        mcp_seen: None,
        delegations_used: 0,
        subtask: None,
    };
    Ok(Arc::new(CliDelegate {
        label: model_label(&delegate.model_id),
        delegate,
        run_id: run_id.to_string(),
        cwd: cwd.to_path_buf(),
        level,
        config,
        store: store.clone(),
        workspace_id: ws.id,
        used: AtomicU32::new(used),
        count: AtomicU32::new(0),
    }))
}

/// 给 CLI 的这一句后面加的一段：有 `delegate_task` 可用、什么时候用。
pub fn prompt_note(d: &CliDelegate) -> String {
    format!(
        "\n\n<peck-delegate>\nYou have an MCP tool `delegate_task` (server `peck`) that hands one self-contained subtask to a cheaper model ({}) working in this same project with the same permissions; it reports back what it did and which files it changed. Use it for mechanical work you can specify completely (renaming across files, updating call sites, writing tests you describe), keep design decisions to yourself, and check what it changed. At most {MAX_DELEGATIONS} per task.\n</peck-delegate>",
        d.label
    )
}

/// `delegate_task` 的说明（本机 MCP 的 tools/list 用）。
pub fn tool_description(d: &CliDelegate) -> String {
    format!(
        "Hand one self-contained subtask to a cheaper model ({}) that works in this same project with the same tools and permissions, then get its report and the list of files it changed. Write the task so it can be done without asking questions: what to change, where, and how to check it. Don't delegate design decisions; review what it changed. At most {MAX_DELEGATIONS} per task.",
        d.label
    )
}

/// 跑一个子任务。回给 CLI 的一段话（出错也是一段话，`Err` 表示这次调用算失败）。
pub async fn run(d: &CliDelegate, sink: &EventSink, task: &str, files: &[String]) -> Result<String, String> {
    let task = task.trim();
    if task.is_empty() {
        return Err("task is required: write what the sub-agent should do.".into());
    }
    if d.used.fetch_add(1, Ordering::SeqCst) >= MAX_DELEGATIONS {
        d.used.fetch_sub(1, Ordering::SeqCst);
        return Err(format!("一个任务最多委派 {MAX_DELEGATIONS} 次，剩下的自己做。"));
    }
    let n = d.count.fetch_add(1, Ordering::SeqCst) + 1;
    let id = format!("peck-sub-{n}");
    let prefix = format!("{id}-");
    let first_line: String = task.lines().next().unwrap_or("").chars().take(60).collect();
    sink.emit(EventBody::SubagentStarted {
        subagent_id: id.clone(),
        parent_call_id: None,
        title: format!("{}：{first_line}", d.label),
        model: Some(d.delegate.model_id.clone()),
    });
    // 旁听子任务：最后说的话是报告，改了哪些文件。
    let report: Arc<Mutex<String>> = Arc::default();
    let changed: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let (r2, c2) = (report.clone(), changed.clone());
    let child = sink.for_delegate(id.clone(), move |e: &AgentEvent| match &e.body {
        EventBody::MessageCompleted { text, .. } if !text.trim().is_empty() => {
            if let Ok(mut r) = r2.lock() {
                *r = text.clone();
            }
        }
        EventBody::ToolCallCompleted { diff, .. } => {
            if let Ok(mut c) = c2.lock() {
                c.extend(diff.iter().map(|f| f.path.clone()));
            }
        }
        _ => {}
    });
    let (tx, rx) = mpsc::unbounded_channel();
    runner::register_sub_control(&d.run_id, &prefix, tx);
    let mut config = d.config.clone();
    config.subtask = Some(prefix.clone());
    let (store, ws) = (d.store.clone(), d.workspace_id.clone());
    let backend = PeckBackend::new(d.delegate.provider.clone(), config, vec![]).on_allow_always(move |rule| {
        if let Ok(s) = store.lock() {
            let _ = s.harness_allow(&ws, rule);
        }
    });
    let prompt = if files.is_empty() { task.to_string() } else { format!("{task}\n\nStart with these files: {}", files.join(", ")) };
    let outcome = backend
        .run(
            AgentTask {
                run_id: d.run_id.clone(),
                prompt,
                cwd: d.cwd.clone(),
                permission: d.level,
                model: Some(d.delegate.model_id.clone()),
                effort: None,
                resume: None,
                peck_mcp: None,
                env: vec![],
                folders: vec![],
            },
            child.clone(),
            rx,
        )
        .await;
    runner::unregister_sub_control(&d.run_id, &prefix);
    let (reason, error) = match outcome {
        Ok(o) => child.delegate_end().unwrap_or((o.reason, None)),
        Err(e) => (TurnEndReason::Error, Some(e.message)),
    };
    let (status, word) = match reason {
        TurnEndReason::Done => (ToolStatus::Ok, "做完了".to_string()),
        TurnEndReason::Interrupted => (ToolStatus::Cancelled, "被停下了".to_string()),
        _ => (ToolStatus::Failed, format!("没做完（{}）", error.unwrap_or_else(|| "出错了".into()))),
    };
    sink.emit(EventBody::SubagentCompleted { subagent_id: id, status });
    let files: Vec<String> = changed.lock().map(|c| c.iter().cloned().collect()).unwrap_or_default();
    let files = if files.is_empty() { "没改文件".to_string() } else { format!("改了：{}", files.join("、")) };
    let report = report.lock().map(|r| r.trim().to_string()).unwrap_or_default();
    let report = if report.is_empty() { "（没写报告）".to_string() } else { super::tools::clip_bytes(&report, 12_000).to_string() };
    Ok(format!("子任务（{}）{word}；{files}。\n它的报告：\n{report}", d.label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::mock::{ScriptTurn, ScriptedModel};
    use serde_json::json;
    use std::time::Duration;

    fn turn(text: &str, calls: Vec<(&str, serde_json::Value)>) -> ScriptTurn {
        ScriptTurn {
            reasoning: None,
            text: (!text.is_empty()).then(|| text.to_string()),
            tool_calls: calls.into_iter().map(|(n, v)| (n.to_string(), v)).collect(),
            usage: crate::gateway::event::TokenUsage { input: Some(800), output: Some(60), ..Default::default() },
        }
    }

    #[tokio::test]
    async fn cli_delegation_runs_a_peck_subtask_and_reports_back() {
        let root = std::env::temp_dir().join(format!("peck-delegate-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(ws_dir.join("a.txt"), "一\n").unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w-del".into(),
            path: ws_dir.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        let store = Arc::new(Mutex::new(store));
        let model = "deepseek/deepseek-flash";
        let child = Arc::new(ScriptedModel::new(
            model,
            vec![
                turn("", vec![("read_file", json!({"path": "a.txt"}))]),
                turn("", vec![("write_file", json!({"path": "a.txt", "content": "二\n"}))]),
                turn("改成二了。", vec![]),
            ],
            Duration::ZERO,
        ));
        let requests = child.requests.clone();
        let d = prepare(&store, Delegate { model_id: model.into(), provider: child, rounds: crate::harness::r#loop::DEFAULT_SUB_ROUNDS }, "run-del", &session.id, &ws_dir, PermissionLevel::Auto, None).unwrap();
        assert!(prompt_note(&d).contains("delegate_task"));
        let (sink, events) = EventSink::memory("claude-code");
        let text = run(&d, &sink, "把 a.txt 改成「二」", &["a.txt".into()]).await.unwrap();
        assert_eq!(std::fs::read_to_string(ws_dir.join("a.txt")).unwrap(), "二\n");
        assert!(text.contains("做完了") && text.contains("改了：a.txt") && text.contains("改成二了"), "{text}");
        let events = events.lock().unwrap().clone();
        // 开头一条 subagent.started（带模型），子任务自己的开头、收尾不发，最后一条 subagent.completed。
        assert!(matches!(&events[0].body, EventBody::SubagentStarted { subagent_id, model: Some(m), .. } if subagent_id == "peck-sub-1" && m == model));
        assert!(matches!(&events.last().unwrap().body, EventBody::SubagentCompleted { status: ToolStatus::Ok, .. }));
        assert!(!events.iter().any(|e| matches!(e.body, EventBody::TurnCompleted { .. } | EventBody::TurnStarted { .. } | EventBody::SessionStarted { .. })));
        let inner: Vec<&AgentEvent> = events[1..events.len() - 1].iter().collect();
        assert!(inner.iter().all(|e| e.subagent_id.as_deref() == Some("peck-sub-1")));
        assert!(inner.iter().any(|e| matches!(&e.body, EventBody::UsageUpdated { model: Some(m), .. } if m == model)));
        // 子任务拿不到问站主、清单、记忆、再委派；这一句里带上了要先看的文件。
        let req = requests.lock().unwrap()[0].clone();
        let names: Vec<String> = req.tools.iter().map(|t| t.name.clone()).collect();
        for banned in ["ask_user", "update_plan", "memory", "delegate_task"] {
            assert!(!names.contains(&banned.to_string()), "{banned}");
        }
        assert!(serde_json::to_string(&req.messages).unwrap().contains("Start with these files: a.txt"));
        // 一个任务最多 6 次（前几句用掉的也算）。
        d.used.store(MAX_DELEGATIONS, Ordering::SeqCst);
        assert!(run(&d, &sink, "再来一个", &[]).await.unwrap_err().contains("最多委派"));
        assert!(run(&d, &sink, "  ", &[]).await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
