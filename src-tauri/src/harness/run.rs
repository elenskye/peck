//! 发 Peck 循环的任务（「谁来干」是 `api:<供应商>/<模型>`）和 ONE SHOT。
//!
//! 和 CLI 引擎的任务（`gateway/runner.rs`）一样：任务记录（`runs.engine` 记 `peck`）、一个项目一次一个、
//! 影子仓库的快照（CHANGES 和撤销照旧能用）、`run_cancel` 能停、审批和回答走 `gateway_control`。
//! 多出来的：一个任务里的对话存着（下一句接着发）、「这个项目总是允许」存着、花费上限按这个月已花的算。
//!
//! ONE SHOT：只读回答（Peck 循环按 PLAN 档、只给读的工具；CLI 引擎按 PLAN 档），不占任务号、不锁项目、
//! 不锁终端、不存快照；事件照样进事件日志（花费算进这个月）。Antigravity 只做直接调用，不进 ONE SHOT。

use super::r#loop::{Budget, LoopConfig, PeckBackend, SpendProbe, DEFAULT_MAX_ROUNDS};
use crate::events::{ChangeSetSummary, EventSink as RunSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{DispatchPlan, ResolvedStepPlan};
use crate::gateway::event::{AgentEvent, EventBody, PermissionLevel, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{catalog, engines, providers, runner, AgentBackend, AgentTask, Control, Message, ModelProvider};
use crate::process::ProcessCancelHandle;
use crate::run::ActiveRunContext;
use crate::snapshot::{record_run_base, record_step_finish, record_step_start, ShadowRepo};
use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

fn permission_of(p: Option<&str>) -> Result<PermissionLevel, String> {
    match p {
        None | Some("edit") => Ok(PermissionLevel::Edit),
        Some("plan") => Ok(PermissionLevel::Plan),
        Some("auto") => Ok(PermissionLevel::Auto),
        Some(_) => Err("权限只有 PLAN、EDIT、AUTO 三档。".into()),
    }
}

/// 命令输出落盘的地方：Peck 数据目录下，每个任务（会话）一个文件夹，ONE SHOT 每次一个。
pub fn spill_root() -> PathBuf {
    crate::mode::app_data_dir().join("harness-output")
}

/// 打开 App 时在后台跑：落盘的输出放了 14 天没动过的整个文件夹删掉。
pub fn prune_spills(root: &Path, max_age: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old && path.is_dir() && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn runtime_dir(workspace: &Path) -> PathBuf {
    crate::mode::app_data_dir()
        .join("harness-runtime")
        .join(crate::workspace::workspace_hash(workspace))
}

/// 这个月（UTC）第一天零点的毫秒时间戳。
pub fn month_start_ms(now_ms: i64) -> i64 {
    let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let first = now.date().replace_day(1).unwrap_or(now.date());
    first.midnight().assume_utc().unix_timestamp() * 1000
}

impl Store {
    /// 这个任务里 Peck 循环的对话（没有就是空的）。
    pub fn harness_conversation(&self, session_id: &str) -> StoreResult<Vec<Message>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT messages FROM harness_conversations WHERE session_id = ?1", params![session_id], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    pub fn save_harness_conversation(&self, session_id: &str, model: &str, messages: &[Message]) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO harness_conversations(session_id, model, messages, updated_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET model = excluded.model, messages = excluded.messages, updated_at = excluded.updated_at",
            params![session_id, model, serde_json::to_string(messages)?, now_ms()],
        )?;
        Ok(())
    }

    /// 这个项目「总是允许」过哪些类。
    pub fn harness_allowed(&self, workspace_id: &str) -> StoreResult<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT rule FROM harness_allow WHERE workspace_id = ?1")?;
        let rows = stmt.query_map(params![workspace_id], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// 所有项目「总是允许」过的（设置 → 权限里列出来、能撤）。
    pub fn harness_allow_all(&self) -> StoreResult<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT workspace_id, rule, created_at FROM harness_allow ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn harness_disallow(&self, workspace_id: &str, rule: &str) -> StoreResult<()> {
        self.conn.execute(
            "DELETE FROM harness_allow WHERE workspace_id = ?1 AND rule = ?2",
            params![workspace_id, rule],
        )?;
        Ok(())
    }

    pub fn harness_allow(&self, workspace_id: &str, rule: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO harness_allow(workspace_id, rule, created_at) VALUES(?1, ?2, ?3)",
            params![workspace_id, rule, now_ms()],
        )?;
        Ok(())
    }

    /// 从 `since`（毫秒）起 Peck 循环（API）花了多少美元。算不出花费的请求不计。
    pub fn harness_spend_since(&self, since: i64) -> StoreResult<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(json_extract(event, '$.cost.usd')), 0.0) FROM agent_events
             WHERE engine = 'peck' AND type = 'usage.updated' AND at >= ?1",
            params![since],
            |r| r.get::<_, f64>(0),
        )?)
    }
}

/// 花费上限：上限 + 这个月已经花的。`cap` 空着或 ≤ 0 就不管。
fn budget(store: &Arc<Mutex<Store>>, cap: Option<f64>) -> Option<Budget> {
    let cap = cap.filter(|c| c.is_finite() && *c > 0.0)?;
    let spent = store.lock().ok().and_then(|s| s.harness_spend_since(month_start_ms(now_ms())).ok()).unwrap_or(0.0);
    Some(Budget { cap_usd: cap, spent_usd: spent })
}

/// 每次请求前现算这个月花了多少（花费上限用）。
fn live_spend(store: &Arc<Mutex<Store>>) -> SpendProbe {
    let store = store.clone();
    Arc::new(move || store.lock().ok().and_then(|s| s.harness_spend_since(month_start_ms(now_ms())).ok()))
}

fn model_id_of(who: &str) -> Result<String, String> {
    let id = who.strip_prefix("api:").ok_or("这个入口只跑 Peck 循环（api:…）。")?;
    catalog::catalog().model(id).ok_or_else(|| format!("模型目录里没有 {id}。"))?;
    Ok(id.to_string())
}

fn model_label(model_id: &str) -> String {
    let cat = catalog::catalog();
    match cat.model(model_id) {
        Some(m) => format!("Peck · {}", m.label),
        None => format!("Peck · {model_id}"),
    }
}

/// 发一个 Peck 循环的任务。返回任务 id；过程经 `channel` 来（统一事件）。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn harness_run_start(
    session_id: String,
    prompt: String,
    who: String,
    permission: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let level = permission_of(permission.as_deref())?;
    let model_id = model_id_of(&who)?;
    let provider = providers::for_model(&state, &model_id).await.map_err(|e| e.message)?;
    start_peck_run(
        PeckRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id,
            prompt,
            who,
            permission,
            level,
            model_id,
            effort,
            cost_cap,
            env: super::tools::bash::shell_env(),
        },
        provider,
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

pub(crate) struct PeckRun {
    pub store: Arc<Mutex<Store>>,
    pub active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>>,
    pub session_id: String,
    pub prompt: String,
    pub who: String,
    pub permission: Option<String>,
    pub level: PermissionLevel,
    pub model_id: String,
    pub effort: Option<String>,
    pub cost_cap: Option<f64>,
    pub env: Vec<(String, String)>,
}

/// 收尾时要的：最后一段回答、回合怎么结束的。
#[derive(Default)]
struct Ending {
    text: String,
    error: Option<String>,
}

pub(crate) fn start_peck_run(
    run: PeckRun,
    provider: Arc<dyn ModelProvider>,
    out: impl Fn(&AgentEvent) + Send + Sync + 'static,
) -> Result<String, String> {
    let PeckRun { store, active_runs, session_id, prompt, who, permission, level, model_id, effort, cost_cap, env } = run;
    let label = model_label(&model_id);
    let (run_id, step_id, workspace, workspace_id, cancel_rx) = {
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
            flow_id: "peck".into(),
            flow_name: label.clone(),
            clean_input: prompt.clone(),
            steps: vec![ResolvedStepPlan {
                id: step_id.clone(),
                step_index: 0,
                agent_id: "peck".into(),
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
        db.set_run_engine(&run_id, "peck").map_err(|e| e.to_string())?;
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
        (run_id, step_id, workspace, ws.id, cancel_rx)
    };
    let (history, allowed) = {
        let db = store.lock().map_err(|e| e.to_string())?;
        (
            db.harness_conversation(&session_id).unwrap_or_default(),
            db.harness_allowed(&workspace_id).unwrap_or_default(),
        )
    };
    let config = LoopConfig {
        model_id: model_id.clone(),
        max_rounds: DEFAULT_MAX_ROUNDS,
        budget: budget(&store, cost_cap),
        live_spend: Some(live_spend(&store)),
        oneshot: false,
        allowed,
        spill_dir: spill_root().join(&session_id),
        runtime_dir: runtime_dir(&workspace),
        env,
        session_ref: Some(format!("peck-{session_id}")),
    };
    let hook_store = store.clone();
    let hook_ws = workspace_id.clone();
    let backend = PeckBackend::new(provider, config, history).on_allow_always(move |rule| {
        if let Ok(s) = hook_store.lock() {
            let _ = s.harness_allow(&hook_ws, rule);
        }
    });

    let (control_tx, control_rx) = mpsc::unbounded_channel();
    runner::register_control(&run_id, control_tx.clone());
    let id = run_id.clone();
    tokio::spawn(async move {
        let ending = Arc::new(Mutex::new(Ending::default()));
        let seen = ending.clone();
        let sink = EventSink::new(id.clone(), Some(step_id.clone()), "peck", Some(store.clone()), move |e: &AgentEvent| {
            if let Ok(mut end) = seen.lock() {
                match &e.body {
                    EventBody::MessageCompleted { text, .. } => end.text = text.clone(),
                    EventBody::TurnCompleted { error, .. } => end.error = error.clone(),
                    _ => {}
                }
            }
            out(e);
        });
        let run_sink = RunSink::Persistent { target: Box::new(RunSink::new_memory().0), store: store.clone() };
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
                agent_id: "peck".into(),
                agent_name: label.clone(),
                title: label.clone(),
                rendered_prompt: String::new(),
                rendered_expects: vec![],
            }
            .to_descriptor(status)
        };
        run_sink.emit(RunEvent::RunStarted {
            run_id: id.clone(),
            flow_id: "peck".into(),
            flow_name: label.clone(),
            steps: vec![descriptor(StepStatus::Pending)],
        });
        run_sink.emit(RunEvent::StepStarted { run_id: id.clone(), step_id: step_id.clone(), step_index: 0, agent_id: "peck".into() });
        run_sink.emit(RunEvent::StepExecution {
            run_id: id.clone(),
            step_id: step_id.clone(),
            mode: "api".into(),
            notice: format!("{label}：Peck 自己的循环和工具箱；要批的事、要问的事会弹卡片。"),
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
        let task = AgentTask {
            run_id: id.clone(),
            prompt,
            cwd: workspace.clone(),
            permission: level,
            model: Some(model_id.clone()),
            effort,
            resume: None,
            peck_mcp: None,
        };
        let outcome = backend.run(task, sink.clone(), control_rx).await;
        watcher.abort();
        runner::unregister_control(&id);
        if let (Ok(conversation), Ok(db)) = (backend.conversation.lock(), store.lock()) {
            let _ = db.save_harness_conversation(&session_id, &model_id, &conversation);
        }

        let (end_text, end_error) = ending.lock().map(|e| (e.text.clone(), e.error.clone())).unwrap_or_default();
        let (step_status, run_status, summary) = match &outcome {
            Ok(o) => match o.reason {
                TurnEndReason::Done => (
                    StepStatus::Succeeded,
                    RunStatus::Succeeded,
                    if end_text.trim().is_empty() { format!("{label} 做完了。") } else { end_text },
                ),
                TurnEndReason::Interrupted | TurnEndReason::Cancelled => (StepStatus::Cancelled, RunStatus::Cancelled, "已停止。".into()),
                TurnEndReason::Limit => (StepStatus::Failed, RunStatus::Failed, end_error.unwrap_or_else(|| "到了上限，先停下。".into())),
                TurnEndReason::Refused => (StepStatus::Failed, RunStatus::Failed, "模型拒绝了这个请求。".into()),
                TurnEndReason::Error => (StepStatus::Failed, RunStatus::Failed, format!("没做完：{}", end_error.unwrap_or_else(|| "出错了。".into()))),
            },
            Err(e) => (StepStatus::Failed, RunStatus::Failed, format!("没做完：{}", e.message)),
        };
        let mut changeset = ChangeSetSummary::default();
        if let (Some(r), Some(before)) = (repo.as_ref(), before.as_ref()) {
            if let Ok(after) = r.create_snapshot(&format!("{step_id}-after")) {
                record_step_finish(&id, &step_id, after.clone());
                changeset = r.diff_changeset(before, &after, &step_id).unwrap_or_default();
            }
        }
        for f in &changeset.files {
            run_sink.emit(RunEvent::FileTouched { run_id: id.clone(), step_id: step_id.clone(), path: f.path.clone(), kind: f.kind.clone() });
        }
        run_sink.emit(RunEvent::StepFinished {
            run_id: id.clone(),
            step_id: step_id.clone(),
            status: step_status,
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            changeset,
        });
        run_sink.emit(RunEvent::RunFinished { run_id: id.clone(), status: run_status, summary });
        if let Ok(mut runs) = active_runs.lock() {
            runs.remove(&id);
        }
    });
    Ok(run_id)
}

/// ONE SHOT：选一个引擎（API 模型或 CLI，Antigravity 除外）只读回答一句。返回 id（`oneshot-…`）。
#[tauri::command]
pub async fn harness_oneshot(
    workspace_id: String,
    prompt: String,
    who: String,
    // CLI 引擎里的模型名（AGENTS 里选的；不填用 CLI 默认）。API 模型不看它。
    model: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let workspace = {
        let db = state.store.lock().map_err(|_| "读不到项目。")?;
        PathBuf::from(db.workspace(&workspace_id).map_err(|e| e.to_string())?.path)
    };
    if !workspace.is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    let id = format!("oneshot-{}", uuid::Uuid::new_v4().simple());
    let out = move |e: &AgentEvent| {
        let _ = channel.send(e.clone());
    };
    let cli_model = if who.starts_with("cli:") { model.filter(|m| !m.trim().is_empty()) } else { None };
    let (backend, engine): (Arc<dyn AgentBackend>, String) = if who.starts_with("api:") {
        let model_id = model_id_of(&who)?;
        let provider = providers::for_model(&state, &model_id).await.map_err(|e| e.message)?;
        let config = LoopConfig {
            model_id,
            max_rounds: 12,
            budget: budget(&state.store, cost_cap),
            live_spend: Some(live_spend(&state.store)),
            oneshot: true,
            allowed: HashSet::new(),
            spill_dir: spill_root().join(&id),
            runtime_dir: runtime_dir(&workspace),
            env: super::tools::bash::shell_env(),
            session_ref: None,
        };
        (Arc::new(PeckBackend::new(provider, config, vec![])), "peck".into())
    } else if let Some(engine) = who.strip_prefix("cli:") {
        if crate::gateway::caps::AgentCaps::for_engine(engine).direct_only {
            return Err("Antigravity 只做直接调用，不进 ONE SHOT。".into());
        }
        (engines::from_state(&state, engine).await.map_err(|e| e.message)?, engine.to_string())
    } else {
        return Err("ONE SHOT 选一个 API 模型或 CLI 引擎。".into());
    };
    start_oneshot(state.store.clone(), id.clone(), engine, workspace, prompt, cli_model, effort, backend, out);
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_oneshot(
    store: Arc<Mutex<Store>>,
    id: String,
    engine: String,
    workspace: PathBuf,
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    backend: Arc<dyn AgentBackend>,
    out: impl Fn(&AgentEvent) + Send + Sync + 'static,
) {
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    runner::register_control(&id, control_tx);
    tokio::spawn(async move {
        let sink = EventSink::new(id.clone(), None, engine, Some(store), out);
        let task = AgentTask {
            run_id: id.clone(),
            prompt,
            cwd: workspace,
            permission: PermissionLevel::Plan,
            model,
            effort,
            resume: None,
            peck_mcp: None,
        };
        let _ = backend.run(task, sink, control_rx).await;
        runner::unregister_control(&id);
    });
}

/// 打开落盘的完整命令输出（只认 Peck 输出目录里的文件，最多 4MB）。
#[tauri::command]
pub async fn harness_output_read(path: String) -> Result<String, String> {
    read_spill(&spill_root(), Path::new(&path))
}

fn read_spill(root: &Path, path: &Path) -> Result<String, String> {
    let root = root.canonicalize().map_err(|_| "还没有落盘的输出。".to_string())?;
    let file = path.canonicalize().map_err(|_| "这份输出不在了。".to_string())?;
    if !file.starts_with(&root) {
        return Err("只能打开 Peck 存下来的命令输出。".into());
    }
    let bytes = std::fs::read(&file).map_err(|_| "读不到这份输出。".to_string())?;
    let limit = 4 * 1024 * 1024;
    let mut text = String::from_utf8_lossy(&bytes[..bytes.len().min(limit)]).into_owned();
    if bytes.len() > limit {
        text.push_str(&format!("\n[只显示了前 4 MB，共 {} MB]", bytes.len() / 1024 / 1024));
    }
    Ok(text)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    /// 这个月 Peck 循环（API）花了多少美元。
    pub month_usd: f64,
    /// 这个月从哪天算起（毫秒）。
    pub since: i64,
}

#[tauri::command]
pub async fn harness_spend(state: tauri::State<'_, AppState>) -> Result<Spend, String> {
    let since = month_start_ms(now_ms());
    let db = state.store.lock().map_err(|_| "读不到花费。")?;
    Ok(Spend { month_usd: db.harness_spend_since(since).map_err(|e| e.to_string())?, since })
}

/// 设置 → 权限：「总是允许」的一条。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowRule {
    pub workspace_id: String,
    /// 项目文件夹（家目录写成 `~`）；项目已经不在列表里时为空。
    pub workspace: Option<String>,
    /// `install-deps`（装依赖）、`long-running`（起常驻进程）。
    pub rule: String,
    pub created_at: i64,
}

fn allow_rules(db: &crate::store::Store) -> Result<Vec<AllowRule>, String> {
    Ok(db
        .harness_allow_all()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(workspace_id, rule, created_at)| AllowRule {
            workspace: db
                .workspace(&workspace_id)
                .ok()
                .map(|w| crate::gateway::config::tilde(std::path::Path::new(&w.path))),
            workspace_id,
            rule,
            created_at,
        })
        .collect())
}

#[tauri::command]
pub fn harness_allow_list(state: tauri::State<'_, AppState>) -> Result<Vec<AllowRule>, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    allow_rules(&db)
}

/// 撤掉一条「总是允许」：下次再碰到又会问。
#[tauri::command]
pub fn harness_allow_remove(workspace_id: String, rule: String, state: tauri::State<'_, AppState>) -> Result<Vec<AllowRule>, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    db.harness_disallow(&workspace_id, &rule).map_err(|e| e.to_string())?;
    allow_rules(&db)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::{ApprovalDecision, Cost, CostSource, TokenUsage, UsageScope};
    use crate::gateway::mock::ScriptedModel;
    use std::time::Duration;

    fn temp_store() -> (PathBuf, crate::store::Store, crate::workspace::Workspace) {
        let root = std::env::temp_dir().join(format!("peck-harness-run-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(ws_dir.join("src")).unwrap();
        std::fs::write(ws_dir.join("src/App.tsx"), "export default () => <h1>Todo</h1>;\n").unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: ws_dir.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            gitignore_configured: false,
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        (root, store, ws)
    }

    #[test]
    fn month_start_and_spend_count_only_this_month_of_peck() {
        // 2026-11-12 08:00 UTC → 2026-11-01 00:00 UTC
        assert_eq!(month_start_ms(1_794_441_600_000 + 8 * 3600 * 1000), 1_793_491_200_000);
        let (root, store, _) = temp_store();
        let usage = |usd: Option<f64>| EventBody::UsageUpdated {
            usage: TokenUsage::default(),
            cost: Cost { usd, source: if usd.is_some() { CostSource::Priced } else { CostSource::Unknown } },
            scope: UsageScope::Turn,
        };
        let store = Arc::new(Mutex::new(store));
        let peck = EventSink::new("r1", None, "peck", Some(store.clone()), |_| {});
        peck.emit(usage(Some(0.25)));
        peck.emit(usage(None));
        peck.emit(usage(Some(0.5)));
        EventSink::new("r2", None, "claude-code", Some(store.clone()), |_| {}).emit(usage(Some(9.0)));
        let s = store.lock().unwrap();
        assert!((s.harness_spend_since(0).unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(s.harness_spend_since(now_ms() + 1000).unwrap(), 0.0);
        drop(s);
        assert_eq!(budget(&store, Some(0.0)), None);
        assert!((budget(&store, Some(5.0)).unwrap().spent_usd - 0.75).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn old_spill_folders_are_pruned() {
        let root = std::env::temp_dir().join(format!("peck-spill-{}", uuid::Uuid::new_v4()));
        let (fresh, stale) = (root.join("sess-new"), root.join("sess-old"));
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("1-bash.log"), "x").unwrap();
        let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        std::fs::File::open(&stale).unwrap().set_modified(month_ago).unwrap();
        assert_eq!(prune_spills(&root, std::time::Duration::from_secs(14 * 86_400)), 1);
        assert!(fresh.exists() && !stale.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversations_and_always_allow_are_stored() {
        let (root, store, _) = temp_store();
        assert!(store.harness_conversation("s1").unwrap().is_empty());
        let msgs = vec![Message { role: crate::gateway::Role::User, parts: vec![crate::gateway::Part::Text { text: "你好".into() }] }];
        store.save_harness_conversation("s1", "deepseek/deepseek-flash", &msgs).unwrap();
        store.save_harness_conversation("s1", "deepseek/deepseek-flash", &msgs).unwrap();
        assert_eq!(store.harness_conversation("s1").unwrap(), msgs);
        store.harness_allow("w1", "install-deps").unwrap();
        store.harness_allow("w1", "install-deps").unwrap();
        assert_eq!(store.harness_allowed("w1").unwrap(), HashSet::from(["install-deps".to_string()]));
        assert!(store.harness_allowed("w2").unwrap().is_empty());
        assert_eq!(store.harness_allow_all().unwrap().len(), 1);
        store.harness_disallow("w1", "install-deps").unwrap();
        assert!(store.harness_allowed("w1").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_spilled_output_can_be_opened() {
        let root = std::env::temp_dir().join(format!("peck-spill-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("run-1")).unwrap();
        std::fs::write(root.join("run-1/1-bash.log"), "全部的输出").unwrap();
        assert_eq!(read_spill(&root, &root.join("run-1/1-bash.log")).unwrap(), "全部的输出");
        assert!(read_spill(&root, &root.join("run-1/../../etc")).is_err());
        assert!(read_spill(&root, Path::new("/etc/hosts")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_peck_task_runs_end_to_end_records_changes_and_resumes() {
        let (root, store, ws) = temp_store();
        let session = store.create_session(&ws.id, None).unwrap();
        let mut store = store;
        let filler = DispatchPlan { flow_id: "x".into(), flow_name: "x".into(), clean_input: "x".into(), steps: vec![] };
        store.create_run(&session.id, "占号", 920_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: Arc<Mutex<HashMap<String, ActiveRunContext>>> = Arc::default();
        let make = |prompt: &str| PeckRun {
            store: store.clone(),
            active_runs: active.clone(),
            session_id: session.id.clone(),
            prompt: prompt.into(),
            who: "api:deepseek/deepseek-flash".into(),
            permission: Some("auto".into()),
            level: PermissionLevel::Auto,
            model_id: "deepseek/deepseek-flash".into(),
            effort: None,
            cost_cap: Some(100.0),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run_id = start_peck_run(make("把标题改成中文"), Arc::new(ScriptedModel::peck_demo(Duration::ZERO)), move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        assert_eq!(run_id, "run-920002");
        assert!(start_peck_run(make("再来"), Arc::new(ScriptedModel::peck_demo(Duration::ZERO)), |_| {}).unwrap_err().contains("一次跑一个"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let e = tokio::time::timeout_at(deadline, rx.recv()).await.expect("跑太久了").expect("事件断了");
            match &e.body {
                EventBody::UserInputRequested { request_id, .. } => {
                    crate::gateway::runner::gateway_control(run_id.clone(), Control::Answer { request_id: request_id.clone(), answer: "先不改".into() })
                        .await
                        .unwrap();
                }
                EventBody::ApprovalRequested { request_id, .. } => {
                    crate::gateway::runner::gateway_control(run_id.clone(), Control::Approve { request_id: request_id.clone(), decision: ApprovalDecision::AllowOnce })
                        .await
                        .unwrap();
                }
                EventBody::TurnCompleted { .. } => break,
                _ => {}
            }
        }
        for _ in 0..200 {
            if active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(active.lock().unwrap().is_empty());
        {
            let db = store.lock().unwrap();
            let record = db.runs(&session.id).unwrap().into_iter().find(|r| r.id == run_id).unwrap();
            assert_eq!(record.status, RunStatus::Succeeded, "{:?}", record.summary);
            assert_eq!(record.who.as_deref(), Some("api:deepseek/deepseek-flash"));
            assert!(record.summary.as_deref().unwrap_or("").contains("待办"));
            assert!(record.files.iter().any(|f| f.path == "src/App.tsx"), "CHANGES 里有改动：{:?}", record.files);
            assert_eq!(db.run_engine(&run_id).unwrap().as_deref(), Some("peck"));
            assert!(db.agent_events(&run_id).unwrap().len() > 30);
            assert_eq!(db.harness_conversation(&session.id).unwrap().len(), 16);
            assert!(db.harness_spend_since(0).unwrap() > 0.0);
        }
        assert!(crate::gateway::runner::gateway_control(run_id, Control::Interrupt).await.is_err(), "结束了就收不到控制");

        // 接着发一句：带着之前的对话
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![], Duration::ZERO));
        let requests = model.requests.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        start_peck_run(make("副标题呢？"), model, move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        while let Some(e) = rx.recv().await {
            if matches!(e.body, EventBody::TurnCompleted { .. }) {
                break;
            }
        }
        assert_eq!(requests.lock().unwrap()[0].messages.len(), 17);
        let _ = std::fs::remove_dir_all(root);
    }
}
