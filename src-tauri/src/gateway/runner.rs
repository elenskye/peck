//! 发任务（Gateway 版）：「谁来干」是 CLI 引擎（模型列表里的 `cli:<引擎>`）的任务走这里。
//! 旧的 `run_dispatch`（按清单起进程）照旧在，接力和规划 + 实施还走它；Peck 自己的循环（`api:…`）
//! 由 Phase 6 的 `harness/` 接（云端）。
//!
//! 和旧的一样的地方：任务记录（runs 表，多记一个 `engine`）、一个项目一次一个任务、影子仓库的快照
//! （CHANGES 和撤销照旧能用）、`run_cancel` 能停。旧的任务事件（`RunEvent`）只发开始、结束这几条进
//! 任务记录；过程是统一事件（`AgentEvent`），经 `channel` 发给界面、存进事件日志。
//!
//! 审批和提问的回答：IPC `gateway_control(runId, control)`。Grok、Antigravity 跑的时候顺手开 Peck 的
//! 本机 MCP（`ask_user`）。用非默认账号跑时，引擎报的额度不写进顶栏 QUOTA 的卡片。

use super::event::{AgentEvent, EventBody, PermissionLevel, TurnEndReason};
use super::log::EventSink;
use super::{accounts, catalog, engines, mcp, AgentTask, Control};
use crate::events::{ChangeSetSummary, EventSink as RunSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{DispatchPlan, ResolvedStepPlan};
use crate::process::ProcessCancelHandle;
use crate::run::ActiveRunContext;
use crate::snapshot::{record_run_base, record_step_finish, record_step_start, ShadowRepo};
use crate::state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

fn controls() -> &'static Mutex<HashMap<String, mpsc::UnboundedSender<Control>>> {
    static C: OnceLock<Mutex<HashMap<String, mpsc::UnboundedSender<Control>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn permission_of(p: Option<&str>) -> Result<PermissionLevel, String> {
    match p {
        None | Some("edit") => Ok(PermissionLevel::Edit),
        Some("plan") => Ok(PermissionLevel::Plan),
        Some("auto") => Ok(PermissionLevel::Auto),
        Some(_) => Err("权限只有 PLAN、EDIT、AUTO 三档。".into()),
    }
}

/// 发一个 CLI 引擎的任务。返回任务 id；过程经 `channel` 来。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn gateway_run_start(
    session_id: String,
    prompt: String,
    who: String,
    permission: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    resume: Option<String>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let level = permission_of(permission.as_deref())?;
    let Some(engine) = who.strip_prefix("cli:").map(str::to_string) else {
        return Err("这个入口只跑 CLI 引擎（cli:…）；接力、规划 + 实施照旧走原来的发任务。".into());
    };
    let label = catalog::catalog()
        .engines
        .iter()
        .find(|e| e.id == engine)
        .map(|e| e.label.clone())
        .ok_or_else(|| format!("不认识的引擎 {engine}。"))?;
    let backend = engines::from_state(&state, &engine).await.map_err(|e| e.message)?;
    let default_account = {
        let store = state.store.lock().map_err(|_| "读不到设置。")?;
        accounts::AccountBook::load(&store).current(&engine) == accounts::DEFAULT_ID
    };
    start_engine_run(
        EngineRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id,
            prompt,
            who,
            permission,
            level,
            engine,
            label,
            model,
            effort,
            resume,
            default_account,
        },
        backend,
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

/// 一个 CLI 引擎任务要的东西（`gateway_run_start` 拼好，测试里直接给）。
pub(crate) struct EngineRun {
    pub store: Arc<Mutex<crate::store::Store>>,
    pub active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>>,
    pub session_id: String,
    pub prompt: String,
    pub who: String,
    pub permission: Option<String>,
    pub level: PermissionLevel,
    pub engine: String,
    pub label: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub resume: Option<String>,
    pub default_account: bool,
}

/// 占任务号、记任务、起引擎（后台跑），返回任务 id。
pub(crate) fn start_engine_run(
    run: EngineRun,
    backend: Arc<dyn super::AgentBackend>,
    out: impl Fn(&AgentEvent) + Send + Sync + 'static,
) -> Result<String, String> {
    let EngineRun {
        store,
        active_runs,
        session_id,
        prompt,
        who,
        permission,
        level,
        engine,
        label,
        model,
        effort,
        resume,
        default_account,
    } = run;

    // 任务记录：和旧的发任务一样占一个号、一个项目一次一个。
    let (run_id, step_id, workspace, cancel_rx) = {
        let mut active = active_runs.lock().map_err(|e| e.to_string())?;
        let mut db = store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace_for_session(&session_id).map_err(|e| e.to_string())?;
        let workspace = PathBuf::from(&ws.path);
        if !workspace.is_dir() {
            return Err("项目文件夹不在了，重新打开一次。".into());
        }
        if active.values().any(|r| r.workspace_path == workspace) {
            return Err("同一个项目一次跑一个任务。".into());
        }
        let number = db.next_run_number().map_err(|e| e.to_string())?;
        let run_id = format!("run-{number}");
        let step_id = format!("{run_id}-step-1");
        let plan = DispatchPlan {
            flow_id: format!("cli-{engine}"),
            flow_name: label.clone(),
            clean_input: prompt.clone(),
            steps: vec![ResolvedStepPlan {
                id: step_id.clone(),
                step_index: 0,
                agent_id: engine.clone(),
                agent_name: label.clone(),
                title: label.clone(),
                rendered_prompt: prompt.clone(),
                rendered_expects: vec![],
            }],
        };
        let mut record = db.create_run(&session_id, &prompt, number, &plan).map_err(|e| e.to_string())?;
        record.who = Some(who.clone());
        record.permission = permission.clone();
        db.save_run(&record).map_err(|e| e.to_string())?;
        db.set_run_engine(&run_id, &engine).map_err(|e| e.to_string())?;
        let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();
        active.insert(
            run_id.clone(),
            ActiveRunContext {
                run_id: run_id.clone(),
                session_id: session_id.clone(),
                workspace_path: workspace.clone(),
                handoff_rel: String::new(),
                plan,
                agents: vec![],
                cancel_handle,
            },
        );
        (run_id, step_id, workspace, cancel_rx)
    };

    let (control_tx, control_rx) = mpsc::unbounded_channel();
    if let Ok(mut c) = controls().lock() {
        c.insert(run_id.clone(), control_tx.clone());
    }
    let id = run_id.clone();
    tokio::spawn(async move {
        let last_text = Arc::new(Mutex::new(String::new()));
        let seen = last_text.clone();
        let mut sink = EventSink::new(id.clone(), Some(step_id.clone()), engine.clone(), Some(store.clone()), move |e: &AgentEvent| {
            if let EventBody::MessageCompleted { text, .. } = &e.body {
                if let Ok(mut t) = seen.lock() {
                    *t = text.clone();
                }
            }
            out(e);
        });
        if !default_account {
            sink = sink.without_quota_sync();
        }
        let run_sink = RunSink::Persistent {
            target: Box::new(RunSink::new_memory().0),
            store: store.clone(),
        };
        let started = Instant::now();
        let repo = ShadowRepo::open_or_init(&workspace).ok();
        let before = repo.as_ref().and_then(|r| {
            let tree = r.create_snapshot(&format!("{id}-base")).ok()?;
            record_run_base(&id, r, tree.clone());
            record_step_start(&id, &step_id, r, tree.clone());
            Some(tree)
        });
        let descriptor = |status| {
            ResolvedStepPlan {
                id: step_id.clone(),
                step_index: 0,
                agent_id: engine.clone(),
                agent_name: label.clone(),
                title: label.clone(),
                rendered_prompt: String::new(),
                rendered_expects: vec![],
            }
            .to_descriptor(status)
        };
        run_sink.emit(RunEvent::RunStarted {
            run_id: id.clone(),
            flow_id: format!("cli-{engine}"),
            flow_name: label.clone(),
            steps: vec![descriptor(StepStatus::Pending)],
        });
        run_sink.emit(RunEvent::StepStarted {
            run_id: id.clone(),
            step_id: step_id.clone(),
            step_index: 0,
            agent_id: engine.clone(),
        });
        run_sink.emit(RunEvent::StepExecution {
            run_id: id.clone(),
            step_id: step_id.clone(),
            mode: "cli".into(),
            notice: format!("调用 {label}（结构化接口）；要批的事、要问的事会弹卡片。"),
        });

        // 停止：`run_cancel` 改成发打断。
        let interrupt = control_tx.clone();
        let mut cancel_rx = cancel_rx;
        let watcher = tokio::spawn(async move {
            while cancel_rx.changed().await.is_ok() {
                if *cancel_rx.borrow() {
                    let _ = interrupt.send(Control::Interrupt);
                    break;
                }
            }
        });
        let peck_mcp = if matches!(engine.as_str(), "grok" | "antigravity") {
            mcp::hub().await.ok().map(|h| h.register(sink.clone()))
        } else {
            None
        };
        let token = peck_mcp.as_ref().map(|m| m.token.clone());
        let task = AgentTask {
            run_id: id.clone(),
            prompt,
            cwd: workspace.clone(),
            permission: level,
            model,
            effort,
            resume,
            peck_mcp,
        };
        let outcome = backend.run(task, sink.clone(), control_rx).await;
        watcher.abort();
        if let (Some(token), Ok(h)) = (token, mcp::hub().await) {
            h.unregister(&token);
        }
        if let Ok(mut c) = controls().lock() {
            c.remove(&id);
        }

        let (step_status, run_status, failure) = match &outcome {
            Ok(o) => match o.reason {
                TurnEndReason::Done => (StepStatus::Succeeded, RunStatus::Succeeded, None),
                TurnEndReason::Interrupted => (StepStatus::Cancelled, RunStatus::Cancelled, None),
                other => (StepStatus::Failed, RunStatus::Failed, Some(format!("{other:?}"))),
            },
            Err(e) => (StepStatus::Failed, RunStatus::Failed, Some(e.message.clone())),
        };
        let mut changeset = ChangeSetSummary::default();
        if let (Some(r), Some(before)) = (repo.as_ref(), before.as_ref()) {
            if let Ok(after) = r.create_snapshot(&format!("{step_id}-after")) {
                record_step_finish(&id, &step_id, after.clone());
                changeset = r.diff_changeset(before, &after, &step_id).unwrap_or_default();
            }
        }
        for f in &changeset.files {
            run_sink.emit(RunEvent::FileTouched {
                run_id: id.clone(),
                step_id: step_id.clone(),
                path: f.path.clone(),
                kind: f.kind.clone(),
            });
        }
        run_sink.emit(RunEvent::StepFinished {
            run_id: id.clone(),
            step_id: step_id.clone(),
            status: step_status,
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            changeset,
        });
        let text = last_text.lock().map(|t| t.clone()).unwrap_or_default();
        run_sink.emit(RunEvent::RunFinished {
            run_id: id.clone(),
            status: run_status,
            summary: match (run_status, failure) {
                (RunStatus::Succeeded, _) if !text.trim().is_empty() => text,
                (RunStatus::Succeeded, _) => format!("{label} 跑完了。"),
                (RunStatus::Cancelled, _) => "已停止。".into(),
                (_, Some(why)) => format!("{label} 没跑完：{why}"),
                _ => format!("{label} 没跑完。"),
            },
        });
        if let Ok(mut runs) = active_runs.lock() {
            runs.remove(&id);
        }
    });
    Ok(run_id)
}

/// 界面 → 引擎：审批、回答、追加、打断。任务已经结束就报错。
#[tauri::command]
pub async fn gateway_control(run_id: String, control: Control) -> Result<(), String> {
    let tx = controls().lock().ok().and_then(|c| c.get(&run_id).cloned());
    match tx {
        Some(tx) => tx.send(control).map_err(|_| "这个任务已经结束了。".into()),
        None => Err("这个任务已经结束了。".into()),
    }
}

impl crate::store::Store {
    /// 任务用的是哪个引擎（schema 7 的 `runs.engine`）。
    pub fn set_run_engine(&self, run_id: &str, engine: &str) -> crate::store::StoreResult<()> {
        self.conn
            .execute("UPDATE runs SET engine = ?1 WHERE id = ?2", rusqlite::params![engine, run_id])?;
        Ok(())
    }

    pub fn run_engine(&self, run_id: &str) -> crate::store::StoreResult<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row("SELECT engine FROM runs WHERE id = ?1", rusqlite::params![run_id], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_levels_and_controls_for_finished_runs() {
        assert_eq!(permission_of(None).unwrap(), PermissionLevel::Edit);
        assert_eq!(permission_of(Some("plan")).unwrap(), PermissionLevel::Plan);
        assert!(permission_of(Some("yolo")).is_err());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(gateway_control("run-none".into(), Control::Interrupt)).unwrap_err();
        assert!(err.contains("结束"));
    }

    #[tokio::test]
    async fn an_engine_task_runs_end_to_end_with_approvals_from_the_ui() {
        let root = std::env::temp_dir().join(format!("peck-runner-e2e-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(ws_dir.join("notes.txt"), "# 笔记\n").unwrap();
        let store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: ws_dir.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            gitignore_configured: false,
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        // 快照登记表是整个进程共用的，别的测试会用 run-105：先占一个大号，这个任务拿到 run-910002。
        let mut store = store;
        let filler = DispatchPlan { flow_id: "x".into(), flow_name: "x".into(), clean_input: "x".into(), steps: vec![] };
        store.create_run(&session.id, "占号", 910_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>> = Arc::default();
        let backend: Arc<dyn super::super::AgentBackend> =
            Arc::new(super::super::mock::ReplayBackend::fixture("codex", std::time::Duration::ZERO).unwrap());
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run = EngineRun {
            store: store.clone(),
            active_runs: active_runs.clone(),
            session_id: session.id.clone(),
            prompt: "读、改、跑".into(),
            who: "cli:codex".into(),
            permission: Some("edit".into()),
            level: PermissionLevel::Edit,
            engine: "codex".into(),
            label: "Codex".into(),
            model: None,
            effort: None,
            resume: None,
            default_account: true,
        };
        let run_id = start_engine_run(run, backend.clone(), move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        assert_eq!(run_id, "run-910002");
        // 同一个项目一次一个。
        let again = EngineRun {
            store: store.clone(),
            active_runs: active_runs.clone(),
            session_id: session.id.clone(),
            prompt: "再来".into(),
            who: "cli:codex".into(),
            permission: None,
            level: PermissionLevel::Edit,
            engine: "codex".into(),
            label: "Codex".into(),
            model: None,
            effort: None,
            resume: None,
            default_account: true,
        };
        assert!(start_engine_run(again, backend, |_| {}).unwrap_err().contains("一次跑一个"));

        let mut approvals = 0;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let e = tokio::time::timeout_at(deadline, rx.recv()).await.expect("跑太久了").expect("事件断了");
            if let EventBody::ApprovalRequested { request_id, .. } = &e.body {
                approvals += 1;
                gateway_control(
                    run_id.clone(),
                    Control::Approve { request_id: request_id.clone(), decision: super::super::event::ApprovalDecision::AllowOnce },
                )
                .await
                .unwrap();
            }
            if matches!(e.body, EventBody::TurnCompleted { .. }) {
                break;
            }
        }
        assert_eq!(approvals, 3);
        // 收尾在后台：等任务从「在跑」里拿掉。
        for _ in 0..100 {
            if active_runs.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(active_runs.lock().unwrap().is_empty());
        let db = store.lock().unwrap();
        let record = db.runs(&session.id).unwrap().into_iter().find(|r| r.id == run_id).unwrap();
        assert_eq!(record.status, RunStatus::Succeeded);
        assert_eq!(record.who.as_deref(), Some("cli:codex"));
        assert_eq!(db.run_engine(&run_id).unwrap().as_deref(), Some("codex"));
        let logged = db.agent_events(&run_id).unwrap();
        assert!(logged.len() > 20, "事件进了日志");
        assert!(logged.iter().any(|e| matches!(e.body, EventBody::ApprovalResolved { .. })));
        drop(db);
        assert!(gateway_control(run_id, Control::Interrupt).await.is_err(), "结束了就收不到控制");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runs_remember_their_engine() {
        let root = std::env::temp_dir().join(format!("peck-runner-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: root.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            gitignore_configured: false,
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        let plan = DispatchPlan {
            flow_id: "cli-codex".into(),
            flow_name: "Codex".into(),
            clean_input: "x".into(),
            steps: vec![],
        };
        store.create_run(&session.id, "x", 1, &plan).unwrap();
        assert_eq!(store.run_engine("run-1").unwrap(), None, "旧任务没有引擎");
        store.set_run_engine("run-1", "codex").unwrap();
        assert_eq!(store.run_engine("run-1").unwrap().as_deref(), Some("codex"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
