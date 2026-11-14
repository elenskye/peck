use super::{
    profiles::{AgentProfile, ModelProfile, Protocol, Provider},
    provider, tools, ApiCall, ApiEvent, ApiState,
};
use crate::{
    events::{ChangeSetSummary, EventSink, RunEvent, RunStatus, StepStatus, StreamType},
    flow::{DispatchPlan, ResolvedStepPlan},
    mode::ApiMode,
    process::ProcessCancelHandle,
    run::ActiveRunContext,
    snapshot::{self, ShadowRepo},
    state::AppState,
    store::{now_ms, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{ipc::Channel, State};
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Limits {
    pub max_rounds: u32,
    pub max_tool_calls: u32,
    pub max_context_bytes: u32,
    pub max_total_tokens: u64,
    pub max_delegations: u32,
    pub max_reworks: u32,
    pub worker_max_rounds: u32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rounds: 12,
            max_tool_calls: 80,
            max_context_bytes: 180_000,
            max_total_tokens: 500_000,
            max_delegations: 4,
            max_reworks: 1,
            worker_max_rounds: 6,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=40).contains(&self.max_rounds)
            || !(1..=200).contains(&self.max_tool_calls)
            || !(8_000..=800_000).contains(&self.max_context_bytes)
            || !(1024..=2_000_000).contains(&self.max_total_tokens)
            || self.max_delegations > 8
            || self.max_reworks > 2
            || !(1..=20).contains(&self.worker_max_rounds)
        {
            return Err("任务预算超出允许范围。".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTask {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub previous_task_id: Option<String>,
    #[serde(default)]
    pub step_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub review: Option<super::supervisor::Review>,
    #[serde(default)]
    pub changeset: ChangeSetSummary,
    #[serde(default)]
    pub worker_model: Option<ModelProfile>,
    #[serde(default)]
    pub worker_settings: Option<AgentProfile>,
    #[serde(default)]
    pub worker_provider: Option<Provider>,
    pub run_id: String,
    pub workspace_path: PathBuf,
    pub mode: ApiMode,
    pub model: ModelProfile,
    pub settings: AgentProfile,
    pub limits: Limits,
    pub scenario: String,
    pub status: String,
    pub round: u32,
    pub messages: Vec<Value>,
    pub tools: Vec<tools::ToolRecord>,
    pub call_ids: Vec<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub summary: Option<String>,
}

#[tauri::command]
pub async fn api_run_start(
    session_id: String,
    prompt: String,
    limits: Limits,
    role: String,
    scenario: String,
    permission: Option<String>,
    channel: Channel<RunEvent>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if !matches!(
        permission.as_deref(),
        None | Some("plan") | Some("edit") | Some("auto")
    ) {
        return Err("权限只有 PLAN、EDIT、AUTO 三档。".into());
    }
    let (ctx, task, provider, rx) =
        prepare(&state, &session_id, &prompt, limits, &role, &scenario)?;
    let run_id = ctx.run_id.clone();
    if permission.is_some() {
        // 这次选的权限档记在任务上（API 模式按档位给工具 v0.6 接）。
        let mut db = state.store.lock().map_err(|e| e.to_string())?;
        if let Ok(mut record) = db.run(&run_id) {
            record.permission = permission;
            let _ = db.save_run(&record);
        }
    }
    let store = state.store.clone();
    let api = state.api.clone();
    let active = state.active_runs.clone();
    tokio::spawn(async move {
        let sink = EventSink::Persistent {
            target: Box::new(EventSink::from_channel(channel)),
            store: store.clone(),
        };
        execute(task, provider, ctx.plan, sink, api.clone(), store, rx).await;
        if let Ok(mut runs) = active.lock() {
            runs.remove(&ctx.run_id);
        }
        if let Ok(mut calls) = api.active.lock() {
            calls.remove(&ctx.run_id);
        }
    });
    Ok(run_id)
}

pub fn prepare(
    state: &AppState,
    session_id: &str,
    prompt: &str,
    limits: Limits,
    role: &str,
    scenario: &str,
) -> Result<(ActiveRunContext, ApiTask, Provider, watch::Receiver<bool>), String> {
    limits.validate()?;
    if !["worker", "supervisor"].contains(&role) {
        return Err("未知 API 执行角色。".into());
    }
    if prompt.trim().is_empty() || prompt.len() > 80_000 {
        return Err("任务内容不能为空，最多约两万汉字。".into());
    }
    if ![
        "success",
        "authentication",
        "rateLimit",
        "disconnect",
        "slow",
        "loop",
        "toolError",
        "rework",
        "takeover",
        "childSlow",
    ]
    .contains(&scenario)
    {
        return Err("未知 Worker 模拟场景。".into());
    }
    let mut active = state.active_runs.lock().map_err(|e| e.to_string())?;
    let mut calls = state.api.active.lock().map_err(|e| e.to_string())?;
    if !calls.is_empty() {
        return Err("API 模式一次跑一个任务，等现在的做完或先停下。".into());
    }
    let mode = *state.api.mode.lock().map_err(|e| e.to_string())?;
    if mode == ApiMode::Real && scenario != "success" {
        return Err("故障场景仅可在模拟模式使用。".into());
    }
    let mut db = state.store.lock().map_err(|e| e.to_string())?;
    let ws = db
        .workspace_for_session(session_id)
        .map_err(|e| e.to_string())?;
    let workspace = PathBuf::from(&ws.path)
        .canonicalize()
        .map_err(|_| "项目文件夹不在了，重新打开一次。")?;
    if active.values().any(|r| r.touches(&[workspace.clone()])) {
        return Err("同一个项目一次跑一个任务。".into());
    }
    let config = db.api_config().map_err(|e| e.to_string())?;
    let (settings, model, provider) = config.selection(role)?;
    let worker = if role == "supervisor" {
        Some(config.selection("worker")?)
    } else {
        None
    };
    if worker.as_ref().is_some_and(|(_, m, _)| !m.tools) {
        return Err("Worker 模型不支持工具调用。".into());
    }
    if !model.tools {
        return Err("所选 Worker 模型不支持工具调用。".into());
    }
    let number = db.next_run_number().map_err(|e| e.to_string())?;
    let run_id = format!("run-{number}");
    let plan = DispatchPlan {
        flow_id: format!("api-{role}"),
        flow_name: format!(
            "{} · {}",
            if role == "supervisor" {
                "规划 + 实施"
            } else {
                "单模型干活"
            },
            model.name
        ),
        clean_input: prompt.into(),
        steps: vec![ResolvedStepPlan {
            id: format!("{run_id}-{role}"),
            step_index: 0,
            agent_id: format!("api-{role}"),
            agent_name: model.name.clone(),
            title: if role == "supervisor" {
                "规划和审查"
            } else {
                "单模型干活"
            }
            .into(),
            rendered_prompt: prompt.into(),
            rendered_expects: vec![],
        }],
    };
    let task = ApiTask {
        id: uuid::Uuid::new_v4().to_string(),
        parent_id: None,
        previous_task_id: None,
        step_id: plan.steps[0].id.clone(),
        title: plan.steps[0].title.clone(),
        paths: vec![],
        acceptance: String::new(),
        attempt: 1,
        review: None,
        changeset: Default::default(),
        worker_model: worker.as_ref().map(|(_, m, _)| m.clone()),
        worker_provider: worker.as_ref().map(|(_, _, p)| p.clone()),
        worker_settings: worker.map(|(a, _, _)| a),
        run_id: run_id.clone(),
        workspace_path: workspace.clone(),
        mode,
        model,
        settings,
        limits,
        scenario: scenario.into(),
        status: "running".into(),
        round: 0,
        messages: vec![json!({"role":"user","content":prompt})],
        tools: vec![],
        call_ids: vec![],
        input_tokens: Some(0),
        output_tokens: Some(0),
        started_at: now_ms(),
        finished_at: None,
        summary: None,
    };
    db.create_run(session_id, prompt, number, &plan)
        .map_err(|e| e.to_string())?;
    if let Err(e) = db.insert_worker_task(&task) {
        let _ = db.record_event(&RunEvent::RunFinished {
            run_id,
            status: RunStatus::Failed,
            summary: "创建 API 任务失败，未发送请求。".into(),
        });
        return Err(e.to_string());
    }
    let (cancel_handle, rx) = ProcessCancelHandle::new();
    let ctx = ActiveRunContext {
        run_id: run_id.clone(),
        session_id: session_id.into(),
        workspace_path: workspace,
        handoff_rel: String::new(),
        plan,
        agents: vec![],
        cancel_handle,
        writable: vec![],
    };
    // Keep API mode locked for the entire tool loop, including time between HTTP sends.
    calls.insert(run_id.clone(), ctx.cancel_handle.tx.clone());
    active.insert(run_id, ctx.clone());
    Ok((ctx, task, provider, rx))
}

#[tauri::command]
pub async fn api_task_history(
    run_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ApiTask>, String> {
    state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .api_tasks(&run_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn api_export_calls(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let value = state
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .export_api_calls()
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    crate::export::save_export(
        &window,
        "导出 Peck 脱敏调用记录",
        &format!("peck-calls-{}.json", now_ms()),
        "JSON",
        "json",
        bytes,
        "保存调用记录失败",
    )
    .await
}

pub(super) fn save(store: &Arc<Mutex<Store>>, task: &ApiTask) -> Result<(), String> {
    store
        .lock()
        .map_err(|e| e.to_string())?
        .save_worker_task(task)
        .map_err(|e| e.to_string())
}

#[derive(Default)]
pub(super) struct RunUsage {
    pub rounds: u32,
    pub tools: u32,
    pub delegations: u32,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

pub async fn execute(
    mut task: ApiTask,
    provider: Provider,
    plan: DispatchPlan,
    sink: EventSink,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    mut cancel: watch::Receiver<bool>,
) {
    sink.emit(RunEvent::RunStarted {
        run_id: task.run_id.clone(),
        flow_id: plan.flow_id,
        flow_name: plan.flow_name,
        steps: plan
            .steps
            .iter()
            .map(|s| s.to_descriptor(StepStatus::Pending))
            .collect(),
    });
    let baseline = (|| {
        let repo = ShadowRepo::open_or_init(&task.workspace_path)?;
        let base = repo.create_snapshot(&format!("{}-base", task.run_id))?;
        snapshot::record_run_base(&task.run_id, &repo, base);
        let mut db = store.lock().map_err(|e| e.to_string())?;
        let run = db.run(&task.run_id).map_err(|e| e.to_string())?;
        db.save_run(&run).map_err(|e| e.to_string())?;
        Ok::<_, String>(repo)
    })();
    match baseline {
        Ok(repo) => {
            let mut usage = RunUsage {
                input_tokens: Some(0),
                output_tokens: Some(0),
                ..Default::default()
            };
            task = execute_task(
                task,
                provider,
                &sink,
                api,
                store.clone(),
                &mut cancel,
                &repo,
                &mut usage,
            )
            .await;
        }
        Err(error) => {
            task.status = "failed".into();
            task.summary = Some(error);
            task.finished_at = Some(now_ms());
            let _ = save(&store, &task);
        }
    }
    let status = match task.status.as_str() {
        "completed" => RunStatus::Succeeded,
        "cancelled" => RunStatus::Cancelled,
        _ => RunStatus::Failed,
    };
    sink.emit(RunEvent::RunFinished {
        run_id: task.run_id,
        status,
        summary: task.summary.unwrap_or_else(|| "API 任务结束。".into()),
    });
}

pub(super) async fn execute_task(
    mut task: ApiTask,
    provider: Provider,
    sink: &EventSink,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    cancel: &mut watch::Receiver<bool>,
    repo: &ShadowRepo,
    usage: &mut RunUsage,
) -> ApiTask {
    let start = std::time::Instant::now();
    let step_index = store
        .lock()
        .ok()
        .and_then(|db| db.run(&task.run_id).ok())
        .and_then(|r| {
            r.steps
                .iter()
                .find(|s| s.id == task.step_id)
                .map(|s| s.step_index)
        })
        .unwrap_or(0);
    sink.emit(RunEvent::StepStarted {
        run_id: task.run_id.clone(),
        step_id: task.step_id.clone(),
        step_index,
        agent_id: format!("api-{}", task.settings.role),
    });
    sink.emit(RunEvent::StepExecution {
        run_id: task.run_id.clone(),
        step_id: task.step_id.clone(),
        mode: if task.mode == ApiMode::Real {
            "api-real"
        } else {
            "api-mock"
        }
        .into(),
        notice: format!(
            "{} · {} · {} · 思考 {} · 第 {} 次任务尝试",
            if task.mode == ApiMode::Real {
                "真实 API"
            } else {
                "模拟模型 · 真实文件工具"
            },
            task.settings.role,
            task.model.api_model,
            if task.settings.thinking {
                task.settings.effort.as_deref().unwrap_or("默认")
            } else {
                "关闭"
            },
            task.attempt
        ),
    });
    let result = async {
        let before = repo.create_snapshot(&format!("{}-before", task.step_id))?;
        snapshot::record_step_start(&task.run_id, &task.step_id, repo, before);
        {
            let mut db = store.lock().map_err(|e| e.to_string())?;
            let run = db.run(&task.run_id).map_err(|e| e.to_string())?;
            db.save_run(&run).map_err(|e| e.to_string())?;
        }
        let watcher = crate::watch::WorkspaceWatcher::start(
            task.workspace_path.clone(),
            task.run_id.clone(),
            task.step_id.clone(),
            sink.clone(),
            crate::watch::DEFAULT_WATCH_INTERVAL,
        );
        let result = run_loop(
            &mut task,
            &provider,
            sink,
            api,
            store.clone(),
            cancel,
            repo,
            usage,
        )
        .await;
        watcher.stop_and_flush().await;
        result
    }
    .await;
    task.status = if *cancel.borrow() {
        "cancelled"
    } else if result.is_ok() {
        "completed"
    } else if result
        .as_ref()
        .err()
        .is_some_and(|e| e.starts_with("预算暂停："))
    {
        "paused"
    } else {
        "failed"
    }
    .into();
    task.summary = Some(match result {
        Ok(text) => text,
        Err(error) => error,
    });
    if *cancel.borrow() {
        task.summary = Some("任务已停止；文件、工具结果和请求次数保留。".into());
    }
    let capture = (|| {
        let after = repo.create_snapshot(&format!("{}-after", task.step_id))?;
        let before = snapshot::saved_snapshot(&task.run_id)
            .and_then(|r| r.steps.get(&task.step_id).map(|s| s.before_tree.clone()))
            .ok_or("任务快照基线缺失。")?;
        task.changeset = repo.diff_changeset(&before, &after, &task.step_id)?;
        snapshot::record_step_finish(&task.run_id, &task.step_id, after);
        Ok::<(), String>(())
    })();
    if let Err(e) = capture {
        task.status = "failed".into();
        task.summary = Some(format!(
            "{}\nDiff 保存失败：{e}",
            task.summary.as_deref().unwrap_or("")
        ));
    }
    task.finished_at = Some(now_ms());
    if let Err(e) = save(&store, &task) {
        task.status = "failed".into();
        task.summary = Some(format!("任务历史保存失败：{e}"));
    }
    let status = match task.status.as_str() {
        "completed" => StepStatus::Succeeded,
        "cancelled" => StepStatus::Cancelled,
        _ => StepStatus::Failed,
    };
    sink.emit(RunEvent::StepFinished {
        run_id: task.run_id.clone(),
        step_id: task.step_id.clone(),
        status,
        exit_code: None,
        duration_ms: start.elapsed().as_millis() as u64,
        changeset: task.changeset.clone(),
    });
    task
}

async fn run_loop(
    task: &mut ApiTask,
    provider: &Provider,
    sink: &EventSink,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    cancel: &mut watch::Receiver<bool>,
    repo: &ShadowRepo,
    usage: &mut RunUsage,
) -> Result<String, String> {
    let step_id = task.step_id.clone();
    let mut definitions = tools::definitions(&provider.protocol);
    if task.settings.role == "supervisor" {
        definitions.extend(super::supervisor::definitions(&provider.protocol));
    }
    let instruction=format!("You are Peck's single coding Worker on macOS. Complete the user's task using the workspace tools, inspect existing instructions/files first, make concrete changes and run proportionate verification. Use pnpm for JavaScript. All paths are relative to the selected workspace. Do not read credentials, query Keychain, edit .git, or access other projects. Never delegate or spawn another model. Do not run git commit/push or install globally. Commands are sandboxed and ephemeral; persistent servers are killed. Explain the actual changes and verification in the final answer, including failures and unknowns. Tool errors are evidence, not success. Maximum {} model rounds and {} tool calls. Do not send tools after the final round; finish with a brief factual report when done.",task.limits.max_rounds,task.limits.max_tool_calls);
    let instruction = if task.settings.role == "supervisor" {
        format!(
            "{}\n{}",
            instruction
                .replace("single coding Worker", "Supervisor")
                .replace(
                    "Never delegate or spawn another model.",
                    "Delegate only using delegate_task; do not call external models."
                ),
            super::supervisor::INSTRUCTIONS
        )
    } else {
        format!("{instruction}\nAllowed write paths: {}. Acceptance criteria: {}. Work only on the supplied goal/context, never delegate.",task.paths.join(", "),task.acceptance)
    };
    loop {
        if *cancel.borrow() {
            return Err("已取消。".into());
        }
        if usage.input_tokens.is_none() || usage.output_tokens.is_none() {
            return Err("预算暂停：整次运行包含未知 usage，不再发送后续请求。".into());
        }
        if usage.rounds >= task.limits.max_rounds
            || (task.parent_id.is_some() && task.round >= task.limits.worker_max_rounds)
        {
            return Err("预算暂停：已达到本任务模型轮数上限。文件与历史已保留；请检查后发起继续任务，不会自动重试。".into());
        }
        if usage
            .input_tokens
            .zip(usage.output_tokens)
            .is_some_and(|(i, o)| i + o >= task.limits.max_total_tokens)
        {
            return Err("预算暂停：已达到累计 token 阈值；不会发送下一轮。".into());
        }
        // Claude 5.1+ signatures bind the prefix, including the system prompt.
        // Append changing budget information after tool results instead of rewriting it.
        let round_instruction = instruction.clone();
        let mut messages = task.messages.clone();
        if provider.protocol != Protocol::Anthropic {
            messages.insert(0, json!({"role":"system","content":round_instruction}));
        }
        let mut body = provider::body(
            &task.model,
            &task.settings,
            &provider.protocol,
            messages.clone(),
            definitions.clone(),
        );
        if provider.protocol == Protocol::Anthropic {
            body["system"] = json!(round_instruction);
        }
        let size = serde_json::to_vec(&body).map_err(|e| e.to_string())?.len();
        // UTF-8 bytes are a conservative upper bound for tokenizer input. No signed blocks are silently dropped.
        if size > task.limits.max_context_bytes as usize
            || size as u64 + task.settings.max_output_tokens as u64
                > task.model.context_window as u64
        {
            return Err(
                "预算暂停：上下文已达到本任务/模型上限，完整工具与思考块仍保存在历史中。".into(),
            );
        }
        // Reserve room for output before sending; missing usage stops the next round below.
        if let Some(used) = usage
            .input_tokens
            .zip(usage.output_tokens)
            .map(|(i, o)| i + o)
        {
            if used + size as u64 + task.settings.max_output_tokens as u64
                > task.limits.max_total_tokens
            {
                return Err("预算暂停：剩余 token 预算不足以覆盖下一轮输入与输出上限。".into());
            }
        }
        task.round += 1;
        usage.rounds += 1;
        let call = ApiCall {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: Some(task.id.clone()),
            run_id: Some(task.run_id.clone()),
            round: Some(task.round),
            mode: task.mode,
            role: task.settings.role.clone(),
            model: task.model.clone(),
            settings: task.settings.clone(),
            prompt: task.messages[0]["content"].as_str().unwrap_or("").into(),
            started_at: now_ms(),
            finished_at: None,
            status: "running".into(),
            budget_counted: false,
            text: String::new(),
            reasoning: String::new(),
            response: None,
            error: None,
        };
        task.call_ids.push(call.id.clone());
        save(&store, task)?;
        store
            .lock()
            .map_err(|e| e.to_string())?
            .insert_api_call(&call)
            .map_err(|e| e.to_string())?;
        sink.emit(RunEvent::StepOutput {
            run_id: task.run_id.clone(),
            step_id: step_id.clone(),
            stream: StreamType::Stdout,
            chunk: format!(
                "\n[API 第 {} / {} 轮 · {}]",
                task.round, task.limits.max_rounds, call.id
            ),
        });
        // Anthropic system instructions are supplied as a native top-level field by request_once.
        if provider.protocol == Protocol::Anthropic {
            messages.insert(0, json!({"role":"system","content":round_instruction}));
        }
        let mut persistence_error = None;
        let mut text_buffer = String::new();
        let mut emitted_at = std::time::Instant::now();
        let call = super::request_once(
            call,
            provider.clone(),
            messages,
            definitions.clone(),
            task.scenario.clone(),
            api.clone(),
            store.clone(),
            cancel.clone(),
            |event| match event {
                ApiEvent::Text { text, .. } => {
                    text_buffer.push_str(&text);
                    if text_buffer.len() >= 600
                        || emitted_at.elapsed() >= std::time::Duration::from_millis(700)
                    {
                        sink.emit(RunEvent::StepOutput {
                            run_id: task.run_id.clone(),
                            step_id: step_id.clone(),
                            stream: StreamType::Stdout,
                            chunk: std::mem::take(&mut text_buffer),
                        });
                        emitted_at = std::time::Instant::now();
                    }
                }
                ApiEvent::PersistenceError { message } => persistence_error = Some(message),
                _ => {}
            },
        )
        .await;
        if !text_buffer.is_empty() {
            sink.emit(RunEvent::StepOutput {
                run_id: task.run_id.clone(),
                step_id: step_id.clone(),
                stream: StreamType::Stdout,
                chunk: text_buffer,
            });
        }
        if let Some(e) = persistence_error {
            usage.input_tokens = None;
            usage.output_tokens = None;
            task.input_tokens = None;
            task.output_tokens = None;
            return Err(e);
        }
        if call.status != "completed" {
            if call.budget_counted {
                task.input_tokens = None;
                task.output_tokens = None;
                usage.input_tokens = None;
                usage.output_tokens = None;
            }
            return Err(call.error.unwrap_or_else(|| "请求已停止。".into()));
        }
        let response = call.response.ok_or("请求缺少响应记录。")?;
        let input = response.usage.input_tokens.map(|n| {
            if provider.protocol == Protocol::Anthropic {
                n + response.usage.cache_read_tokens.unwrap_or(0)
                    + response.usage.cache_creation_tokens.unwrap_or(0)
            } else {
                n
            }
        });
        usage.input_tokens = usage.input_tokens.zip(input).map(|(a, b)| a + b);
        usage.output_tokens = usage
            .output_tokens
            .zip(response.usage.output_tokens)
            .map(|(a, b)| a + b);
        task.input_tokens = task.input_tokens.zip(input).map(|(a, b)| a + b);
        task.output_tokens = task
            .output_tokens
            .zip(response.usage.output_tokens)
            .map(|(a, b)| a + b);
        let tool_calls = tools::calls(&response.message, &provider.protocol)?;
        // Save the exact provider message, including DeepSeek reasoning_content / Anthropic signatures.
        task.messages.push(response.message);
        save(&store, task)?;
        if !matches!(
            response.stop_reason.as_deref(),
            Some("stop" | "end_turn" | "tool_calls" | "tool_use")
        ) {
            return Err(format!(
                "模型非正常结束（{}）；未执行不完整工具或标记项目完成。",
                response.stop_reason.as_deref().unwrap_or("未知")
            ));
        }
        if tool_calls.is_empty() {
            if task.tools.last().is_some_and(|t| t.status == "failed") {
                return Err("最后工具未成功，未将任务标记完成。".into());
            }
            if response.text.trim().is_empty() {
                return Err("模型没有返回工具或最终说明。".into());
            }
            if task
                .tools
                .iter()
                .rev()
                .find(|t| t.tool.name == "run_command")
                .is_some_and(|t| t.status != "completed")
            {
                return Err(format!(
                    "最后一次命令未成功，任务未通过验证。模型总结：{}",
                    response.text
                ));
            }
            if task.settings.role == "supervisor" {
                super::supervisor::verify_completion(task, &store)?;
            }
            return Ok(response.text);
        }
        if !matches!(
            response.stop_reason.as_deref(),
            Some("tool_calls" | "tool_use")
        ) {
            return Err("工具块与结束原因不一致，未执行工具。".into());
        }
        if usage.tools as usize + tool_calls.len() > task.limits.max_tool_calls as usize {
            return Err("预算暂停：下一批工具超出工具次数上限，未执行本批操作。".into());
        }
        let mut results = vec![];
        for tool in tool_calls {
            if *cancel.borrow() {
                return Err("已取消，未执行后续工具。".into());
            }
            if usage.tools >= task.limits.max_tool_calls {
                return Err("预算暂停：整次运行工具次数已用完。".into());
            }
            usage.tools += 1;
            let mut record = tools::ToolRecord {
                call_id: call.id.clone(),
                tool,
                status: "running".into(),
                output: String::new(),
                started_at: now_ms(),
                finished_at: None,
            };
            task.tools.push(record.clone());
            // An interrupted running tool is never replayed on restart.
            save(&store, task)?;
            sink.emit(RunEvent::StepOutput {
                run_id: task.run_id.clone(),
                step_id: step_id.clone(),
                stream: StreamType::Stdout,
                chunk: format!(
                    "[工具 {}] {}",
                    record.tool.name,
                    tools::clipped(record.tool.arguments.to_string(), 1500)
                ),
            });
            let result = match record.tool.name.as_str() {
                "delegate_task" => {
                    super::supervisor::delegate(
                        task,
                        &record.tool.arguments,
                        sink,
                        api.clone(),
                        store.clone(),
                        cancel,
                        repo,
                        usage,
                    )
                    .await
                }
                "review_task" => {
                    super::supervisor::review(task, &record.tool.arguments, &store, sink)
                }
                _ => {
                    tools::execute(
                        &task.workspace_path,
                        &record.tool,
                        &task.paths,
                        cancel,
                        |stream, chunk| {
                            sink.emit(RunEvent::StepOutput {
                                run_id: task.run_id.clone(),
                                step_id: step_id.clone(),
                                stream,
                                chunk,
                            })
                        },
                    )
                    .await
                }
            };
            record.finished_at = Some(now_ms());
            match result {
                Ok(output) => {
                    record.status = if *cancel.borrow() {
                        "cancelled"
                    } else if ["run_command", "delegate_task"].contains(&record.tool.name.as_str())
                        && serde_json::from_str::<Value>(&output)
                            .ok()
                            .is_some_and(|v| v["status"] != "completed")
                    {
                        "failed"
                    } else {
                        "completed"
                    }
                    .into();
                    record.output = output;
                }
                Err(error) => {
                    record.status = "failed".into();
                    record.output = error;
                }
            }
            *task.tools.last_mut().unwrap() = record.clone();
            save(&store, task)?;
            sink.emit(RunEvent::StepOutput {
                run_id: task.run_id.clone(),
                step_id: step_id.clone(),
                stream: if record.status == "completed" {
                    StreamType::Stdout
                } else {
                    StreamType::Stderr
                },
                chunk: format!(
                    "[{} · {}] {}",
                    record.tool.name,
                    record.status,
                    tools::clipped(record.output.clone(), 2000)
                ),
            });
            results.push(record);
        }
        tools::append_results(&mut task.messages, &provider.protocol, &results);
        task.messages.push(json!({"role":"user","content":format!(
            "Peck runtime budget: remaining run requests: {}; remaining requests for this task: {}. Finish with a factual report before the limit.",
            task.limits.max_rounds.saturating_sub(usage.rounds),
            if task.parent_id.is_some() {
                task.limits.worker_max_rounds.saturating_sub(task.round)
            } else {
                task.limits.max_rounds.saturating_sub(usage.rounds)
            }
        )}));
        save(&store, task)?;
        if task.input_tokens.is_none() || task.output_tokens.is_none() {
            return Err("预算暂停：供应商未返回完整 usage，无法核对 token 预算；已保留工具结果，不自动继续。".into());
        }
    }
}

// Deterministic native mock drives the same persisted loop and real workspace tools.
pub(super) async fn mock_response(
    protocol: &Protocol,
    role: &str,
    round: u32,
    scenario: &str,
    messages: &[Value],
    cancel: &mut watch::Receiver<bool>,
    mut emit: impl FnMut(provider::Delta),
) -> Result<provider::Response, provider::Failure> {
    tokio::select! {biased;_=cancel.changed()=>return Err(provider::Failure::Cancelled),_=tokio::time::sleep(std::time::Duration::from_millis(160))=>{}}
    let calls = if role == "supervisor" && scenario != "loop" {
        super::supervisor::mock_calls(round, scenario, messages)
    } else if scenario == "workerFailure" {
        if round == 1 {
            vec![("run_command", json!({"command":"exit 7"}))]
        } else {
            vec![]
        }
    } else if scenario == "loop" {
        vec![("list_directory", json!({"path":"."}))]
    } else if scenario == "toolError" && round == 1 {
        vec![("read_file", json!({"path":"../outside"}))]
    } else {
        match round {
            1 => vec![
                ("list_directory", json!({"path":"."})),
                ("search", json!({"query":"Peck"})),
            ],
            2 => vec![(
                "apply_patch",
                json!({"edits":[{"path":"peck-worker-demo.txt","oldText":if scenario=="revision"{"Peck 模拟 Worker 文件工具验证\n"}else{""},"newText":if scenario=="revision"{"Peck 返工完成\n"}else{"Peck 模拟 Worker 文件工具验证\n"}}]}),
            )],
            3 => vec![
                ("read_file", json!({"path":"peck-worker-demo.txt"})),
                (
                    "run_command",
                    json!({"command":"test -f peck-worker-demo.txt && wc -l peck-worker-demo.txt"}),
                ),
            ],
            _ => vec![],
        }
    };
    let text = if calls.is_empty() {
        if scenario == "toolError" {
            "模拟模型已收到工具错误；未声称越界读取成功。"
        } else {
            "模拟流程结束。文件与命令结果请查看工具历史；这不是台账项目或真实 AI 开发成果。"
        }
    } else {
        "正在使用工作区工具。"
    };
    emit(provider::Delta::Text(text.into()));
    let message = match protocol {
        Protocol::Responses => {
            let mut output = vec![json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]})];
            output.extend(calls.iter().enumerate().map(|(i,(name,args))|json!({"type":"function_call","call_id":format!("mock-{round}-{i}"),"name":name,"arguments":args.to_string()})));
            json!({"output":output})
        }
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
            let mut m = json!({"role":"assistant","content":text,"reasoning_content":if role=="supervisor"{"SUPERVISOR_PRIVATE_REASONING_SENTINEL"}else{"模拟思考摘要"}});
            if !calls.is_empty() {
                m["tool_calls"]=json!(calls.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("mock-{round}-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>());
            }
            m
        }
        Protocol::Anthropic => {
            let mut blocks = vec![
                json!({"type":"thinking","thinking":if role=="supervisor"{"SUPERVISOR_PRIVATE_REASONING_SENTINEL"}else{"模拟摘要"},"signature":"mock-signature"}),
                json!({"type":"text","text":text}),
            ];
            blocks.extend(calls.iter().enumerate().map(|(i,(name,args))|json!({"type":"tool_use","id":format!("mock-{round}-{i}"),"name":name,"input":args})));
            json!({"role":"assistant","content":blocks})
        }
    };
    Ok(provider::Response {
        text: text.into(),
        reasoning: "模拟摘要".into(),
        message,
        usage: provider::Usage {
            input_tokens: Some(200),
            output_tokens: Some(70),
            ..Default::default()
        },
        stop_reason: Some(
            match protocol {
                Protocol::Anthropic => {
                    if calls.is_empty() {
                        "end_turn"
                    } else {
                        "tool_use"
                    }
                }
                Protocol::Responses | Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
                    if calls.is_empty() {
                        "stop"
                    } else {
                        "tool_calls"
                    }
                }
            }
            .into(),
        ),
        response_model: Some("peck-native-mock".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Each isolated database starts at run-105, while the snapshot registry is process-wide.
    static WORKER_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn native_mock_loop_persists_tools_calls_diff_and_redacted_export() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("peck-worker-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let path = root.join("peck.db");
        let db = Store::open(&path).unwrap();
        let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, None).unwrap();
        let state = AppState::new(db).unwrap();
        let (ctx, task, provider, rx) = prepare(
            &state,
            &session.id,
            "PRIVATE_PROMPT_SENTINEL",
            Limits::default(),
            "worker",
            "success",
        )
        .unwrap();
        assert!(prepare(
            &state,
            &session.id,
            "concurrent",
            Limits::default(),
            "worker",
            "success"
        )
        .is_err());
        let (memory, _) = EventSink::new_memory();
        let sink = EventSink::Persistent {
            target: Box::new(memory),
            store: state.store.clone(),
        };
        execute(
            task,
            provider,
            ctx.plan,
            sink,
            state.api.clone(),
            state.store.clone(),
            rx,
        )
        .await;
        let record = state.store.lock().unwrap().run(&ctx.run_id).unwrap();
        assert_eq!(record.status, RunStatus::Succeeded, "{:?}", record.summary);
        let task = state
            .store
            .lock()
            .unwrap()
            .worker_task(&ctx.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(task.round, 4);
        assert_eq!(task.call_ids.len(), 4);
        assert_eq!(task.tools.len(), 5);
        assert!(task.tools.iter().all(|t| t.status == "completed"));
        let export = state.store.lock().unwrap().export_api_calls().unwrap();
        let raw = export.to_string();
        assert!(!raw.contains("PRIVATE_PROMPT_SENTINEL"));
        assert!(!raw.contains("reasoning_content"));
        assert!(!raw.contains("arguments"));
        assert_eq!(export["calls"].as_array().unwrap().len(), 4);
        assert_eq!(export["calls"][0]["round"], 1);
        assert_eq!(export["calls"][0]["mode"], "mock");
        let snapshot = state
            .store
            .lock()
            .unwrap()
            .snapshot(&ctx.run_id)
            .unwrap()
            .unwrap();
        drop(state);
        std::fs::write(workspace.join("peck-worker-demo.txt"), "later change").unwrap();
        let db = Store::open(&path).unwrap();
        let loaded = db.worker_task(&ctx.run_id).unwrap().unwrap();
        assert_eq!(loaded.tools.len(), 5);
        snapshot::restore_snapshot(snapshot);
        let diff =
            snapshot::get_diff_for_run(&ctx.run_id, None, Some("peck-worker-demo.txt"), None)
                .unwrap();
        assert!(diff.unified_diff.contains("Peck 模拟 Worker"));
        assert!(!diff.content_after.unwrap().contains("later change"));
    }

    #[tokio::test]
    async fn loop_limits_and_interrupted_tools_never_replay() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("peck-worker-limit-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let path = root.join("peck.db");
        let db = Store::open(&path).unwrap();
        let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, None).unwrap();
        let state = AppState::new(db).unwrap();
        let (ctx, mut task, provider, rx) = prepare(
            &state,
            &session.id,
            "limited",
            Limits {
                max_rounds: 2,
                ..Default::default()
            },
            "worker",
            "loop",
        )
        .unwrap();
        let (memory, _) = EventSink::new_memory();
        let sink = EventSink::Persistent {
            target: Box::new(memory),
            store: state.store.clone(),
        };
        execute(
            task.clone(),
            provider,
            ctx.plan,
            sink,
            state.api.clone(),
            state.store.clone(),
            rx,
        )
        .await;
        let saved = state
            .store
            .lock()
            .unwrap()
            .worker_task(&ctx.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, "paused");
        assert_eq!(saved.round, 2);
        assert_eq!(saved.tools.len(), 2);
        assert_eq!(state.store.lock().unwrap().api_calls().unwrap().len(), 2);
        // Simulate a crash after a persisted tool start, with no result acknowledgment.
        task.status = "running".into();
        task.tools.push(tools::ToolRecord{call_id:"unknown".into(),tool:tools::ToolCall{id:"t-unknown".into(),name:"apply_patch".into(),arguments:json!({"edits":[{"path":"never.txt","oldText":"","newText":"must not replay"}]})},status:"running".into(),output:String::new(),started_at:1,finished_at:None});
        save(&state.store, &task).unwrap();
        drop(state);
        let mut db = Store::open(&path).unwrap();
        db.recover_interrupted_runs().unwrap();
        db.recover_api_calls().unwrap();
        db.recover_worker_tasks().unwrap();
        let recovered = db.worker_task(&ctx.run_id).unwrap().unwrap();
        assert_eq!(recovered.status, "interrupted");
        assert_eq!(recovered.tools.last().unwrap().status, "interrupted");
        assert!(!workspace.join("never.txt").exists());
        assert_eq!(db.api_calls().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn tool_budget_and_cancellation_stop_before_more_actions() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        for scenario in ["tool-limit", "cancel"] {
            let root =
                std::env::temp_dir().join(format!("peck-worker-stop-{}", uuid::Uuid::new_v4()));
            let workspace = root.join("project");
            std::fs::create_dir_all(&workspace).unwrap();
            let db = Store::open(&root.join("peck.db")).unwrap();
            let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
            db.save_workspace(&ws).unwrap();
            let session = db.create_session(&ws.id, None).unwrap();
            let state = AppState::new(db).unwrap();
            let (ctx, task, provider, rx) = prepare(
                &state,
                &session.id,
                "bounded",
                Limits {
                    max_tool_calls: 1,
                    ..Default::default()
                },
                "worker",
                if scenario == "cancel" {
                    "slow"
                } else {
                    "success"
                },
            )
            .unwrap();
            let (memory, _) = EventSink::new_memory();
            let sink = EventSink::Persistent {
                target: Box::new(memory),
                store: state.store.clone(),
            };
            let cancel = ctx.cancel_handle.clone();
            let run_id = ctx.run_id.clone();
            let api = state.api.clone();
            let store = state.store.clone();
            let runner = tokio::spawn(async move {
                execute(task, provider, ctx.plan, sink, api, store, rx).await
            });
            if scenario == "cancel" {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                cancel.cancel();
            }
            runner.await.unwrap();
            let saved = state
                .store
                .lock()
                .unwrap()
                .worker_task(&run_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                saved.status,
                if scenario == "cancel" {
                    "cancelled"
                } else {
                    "paused"
                },
                "{:?}",
                saved.summary
            );
            assert!(saved.tools.is_empty());
            assert!(saved.call_ids.len() <= 1);
            assert!(!workspace.join("peck-worker-demo.txt").exists());
        }
    }
    #[test]
    fn prepare_preserves_regional_worker_destination() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("peck-provider-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let db = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, None).unwrap();
        let mut config = super::super::profiles::Config::default();
        config.aliyun_region = super::super::profiles::AliyunRegion::Singapore;
        config.aliyun_workspace_id = "worker-fixture".into();
        config.models[1].provider_id = "aliyun".into();
        config.models[1].efforts.clear();
        config.agents[1].effort = None;
        db.save_api_config(&config).unwrap();
        let state = AppState::new(db).unwrap();
        let (_, task, _, _) = prepare(&state, &session.id, "fixture", Limits::default(), "supervisor", "success").unwrap();
        let saved: ApiTask = serde_json::from_value(serde_json::to_value(task).unwrap()).unwrap();
        config.aliyun_region = super::super::profiles::AliyunRegion::Beijing;
        config.aliyun_workspace_id.clear();
        state.store.lock().unwrap().save_api_config(&config).unwrap();
        assert_eq!(saved.worker_provider.unwrap().base_url,
            "https://worker-fixture.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1");
    }

    #[tokio::test]
    async fn supervisor_delegates_reviews_reworks_and_takes_over_with_original_diffs() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        for scenario in ["success", "rework", "takeover"] {
            let root = std::env::temp_dir().join(format!("peck-team-{}", uuid::Uuid::new_v4()));
            let workspace = root.join("project");
            std::fs::create_dir_all(&workspace).unwrap();
            let path = root.join("peck.db");
            let db = Store::open(&path).unwrap();
            let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
            db.save_workspace(&ws).unwrap();
            let session = db.create_session(&ws.id, None).unwrap();
            let state = AppState::new(db).unwrap();
            let (ctx, task, provider, rx) = prepare(
                &state,
                &session.id,
                "Cooperative concrete task",
                Limits {
                    max_rounds: 20,
                    max_delegations: 2,
                    ..Default::default()
                },
                "supervisor",
                scenario,
            )
            .unwrap();
            let (memory, events) = EventSink::new_memory();
            let sink = EventSink::Persistent {
                target: Box::new(memory),
                store: state.store.clone(),
            };
            execute(
                task,
                provider,
                ctx.plan,
                sink,
                state.api.clone(),
                state.store.clone(),
                rx,
            )
            .await;
            let db = state.store.lock().unwrap();
            let run = db.run(&ctx.run_id).unwrap();
            assert_eq!(
                run.status,
                RunStatus::Succeeded,
                "{scenario}: {:?}",
                run.summary
            );
            let tasks = db.api_tasks(&ctx.run_id).unwrap();
            assert_eq!(tasks.len(), if scenario == "rework" { 3 } else { 2 });
            assert_eq!(run.steps.len(), tasks.len());
            assert_eq!(db.runs(&session.id).unwrap().len(), 1);
            let parent = tasks.iter().find(|t| t.parent_id.is_none()).unwrap();
            let children = tasks
                .iter()
                .filter(|t| t.parent_id.is_some())
                .collect::<Vec<_>>();
            assert!(children
                .iter()
                .all(|t| t.parent_id.as_deref() == Some(&parent.id)
                    && t.settings.role == "worker"
                    && t.worker_model.is_none()));
            assert!(children.iter().all(|t| !serde_json::to_string(&t.messages)
                .unwrap()
                .contains("SUPERVISOR_PRIVATE_REASONING_SENTINEL")));
            assert!(serde_json::to_string(&parent.messages)
                .unwrap()
                .contains("SUPERVISOR_PRIVATE_REASONING_SENTINEL"));
            let calls = db.api_calls().unwrap();
            assert!(calls
                .iter()
                .any(|c| c.role == "supervisor" && c.model.api_model == "claude-opus-5-5"));
            assert!(calls
                .iter()
                .any(|c| c.role == "worker" && c.model.api_model == "deepseek-flash"));
            assert!(calls
                .iter()
                .all(|c| c.run_id.as_deref() == Some(&ctx.run_id) && c.budget_counted));
            assert!(events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, RunEvent::StepAdded { .. })));
            assert!(run.step_files.contains_key(&children[0].step_id));
            if scenario == "rework" {
                assert_eq!(children[0].review.as_ref().unwrap().decision, "rework");
                assert_eq!(
                    children[1].previous_task_id.as_deref(),
                    Some(children[0].id.as_str())
                );
                assert_eq!(children[1].attempt, 2);
                assert_eq!(children[1].review.as_ref().unwrap().decision, "accept");
                assert!(
                    std::fs::read_to_string(workspace.join("peck-worker-demo.txt"))
                        .unwrap()
                        .contains("返工完成")
                );
            } else if scenario == "takeover" {
                assert_eq!(children[0].status, "failed");
                assert_eq!(children[0].review.as_ref().unwrap().decision, "takeover");
                assert!(parent
                    .tools
                    .iter()
                    .any(|t| t.tool.name == "run_command" && t.status == "completed"));
                assert!(workspace.join("peck-supervisor-demo.txt").exists());
            } else {
                assert_eq!(children[0].review.as_ref().unwrap().decision, "accept");
                assert_eq!(calls.len(), 7);
            }
            let child_step = children[0].step_id.clone();
            let snapshot = db.snapshot(&ctx.run_id).unwrap().unwrap();
            drop(db);
            drop(state);
            let db = Store::open(&path).unwrap();
            assert_eq!(db.api_tasks(&ctx.run_id).unwrap().len(), tasks.len());
            snapshot::restore_snapshot(snapshot);
            if scenario != "takeover" {
                std::fs::write(workspace.join("peck-worker-demo.txt"), "changed later").unwrap();
                let diff = snapshot::get_diff_for_run(
                    &ctx.run_id,
                    Some(&child_step),
                    Some("peck-worker-demo.txt"),
                    None,
                )
                .unwrap();
                assert!(diff.content_after.unwrap().contains("Peck 模拟 Worker"));
            }
        }
    }

    #[tokio::test]
    async fn shared_budget_and_child_cancellation_stop_all_roles_without_replay() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        for scenario in ["budget", "cancel"] {
            let root =
                std::env::temp_dir().join(format!("peck-team-stop-{}", uuid::Uuid::new_v4()));
            let workspace = root.join("project");
            std::fs::create_dir_all(&workspace).unwrap();
            let path = root.join("peck.db");
            let db = Store::open(&path).unwrap();
            let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
            db.save_workspace(&ws).unwrap();
            let session = db.create_session(&ws.id, None).unwrap();
            let state = AppState::new(db).unwrap();
            let (ctx, task, provider, rx) = prepare(
                &state,
                &session.id,
                "stop team",
                Limits {
                    max_rounds: if scenario == "budget" { 2 } else { 12 },
                    ..Default::default()
                },
                "supervisor",
                if scenario == "cancel" {
                    "childSlow"
                } else {
                    "success"
                },
            )
            .unwrap();
            let run_id = ctx.run_id.clone();
            let handle = ctx.cancel_handle.clone();
            let store = state.store.clone();
            let api = state.api.clone();
            let (memory, _) = EventSink::new_memory();
            let sink = EventSink::Persistent {
                target: Box::new(memory),
                store: store.clone(),
            };
            let runner = tokio::spawn(async move {
                execute(task, provider, ctx.plan, sink, api, store, rx).await
            });
            if scenario == "cancel" {
                for _ in 0..80 {
                    if state
                        .store
                        .lock()
                        .unwrap()
                        .api_tasks(&run_id)
                        .unwrap()
                        .iter()
                        .any(|t| t.parent_id.is_some() && !t.call_ids.is_empty())
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                handle.cancel();
            }
            runner.await.unwrap();
            let db = state.store.lock().unwrap();
            let tasks = db.api_tasks(&run_id).unwrap();
            assert_eq!(tasks.len(), 2);
            assert!(tasks.iter().all(|t| t.status != "running"));
            if scenario == "budget" {
                assert_eq!(db.api_calls().unwrap().len(), 2);
                assert!(tasks.iter().all(|t| t.status == "paused"));
            } else {
                assert!(tasks.iter().all(|t| t.status == "cancelled"));
            }
            assert!(!workspace.join("peck-worker-demo.txt").exists());
            drop(db);
            drop(state);
            let mut db = Store::open(&path).unwrap();
            let count = db.api_calls().unwrap().len();
            db.recover_worker_tasks().unwrap();
            db.recover_api_calls().unwrap();
            db.recover_interrupted_runs().unwrap();
            assert_eq!(db.api_calls().unwrap().len(), count);
        }
    }

    #[tokio::test]
    async fn delegation_review_and_retry_guards_do_not_send_requests() {
        let _guard = WORKER_TEST_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("peck-team-guards-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("project");
        std::fs::create_dir_all(&workspace).unwrap();
        let db = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::open_workspace(workspace.to_str().unwrap()).unwrap();
        db.save_workspace(&ws).unwrap();
        let session = db.create_session(&ws.id, None).unwrap();
        let state = AppState::new(db).unwrap();
        let (_ctx, parent, _provider, mut rx) = prepare(
            &state,
            &session.id,
            "guards",
            Limits {
                max_delegations: 0,
                max_reworks: 0,
                ..Default::default()
            },
            "supervisor",
            "success",
        )
        .unwrap();
        let repo = ShadowRepo::open_or_init(&workspace).unwrap();
        let (sink, _) = EventSink::new_memory();
        let mut usage = RunUsage::default();
        assert!(super::super::supervisor::delegate(
            &parent,
            &json!({}),
            &sink,
            state.api.clone(),
            state.store.clone(),
            &mut rx,
            &repo,
            &mut usage
        )
        .await
        .is_err());
        let mut child = parent.clone();
        child.id = "guard-child".into();
        child.parent_id = Some(parent.id.clone());
        child.settings.role = "worker".into();
        child.status = "failed".into();
        child.worker_model = None;
        child.worker_settings = None;
        state
            .store
            .lock()
            .unwrap()
            .insert_worker_task(&child)
            .unwrap();
        for decision in ["accept", "rework"] {
            assert!(super::super::supervisor::review(
                &parent,
                &json!({"taskId":child.id,"decision":decision,"feedback":"evidence"}),
                &state.store,
                &sink
            )
            .is_err());
        }
        assert!(super::super::supervisor::delegate(
            &child,
            &json!({}),
            &sink,
            state.api.clone(),
            state.store.clone(),
            &mut rx,
            &repo,
            &mut usage
        )
        .await
        .is_err());
        super::super::supervisor::review(
            &parent,
            &json!({"taskId":child.id,"decision":"takeover","feedback":"take over explicitly"}),
            &state.store,
            &sink,
        )
        .unwrap();
        assert!(super::super::supervisor::verify_completion(&parent, &state.store).is_err());
        let mut parent = parent;
        let provider = super::super::profiles::providers().remove(0);
        let error = run_loop(
            &mut parent,
            &provider,
            &sink,
            state.api.clone(),
            state.store.clone(),
            &mut rx,
            &repo,
            &mut RunUsage::default(),
        )
        .await
        .unwrap_err();
        assert!(error.starts_with("预算暂停："));
        assert!(state.store.lock().unwrap().api_calls().unwrap().is_empty());
    }
}
