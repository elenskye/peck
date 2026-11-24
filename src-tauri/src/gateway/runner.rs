//! CLI 引擎的任务（「谁来干」是 `cli:<引擎>`）。Peck Loop（`api:…`）的在 `harness/run.rs`，两边共用 `run.rs` 的
//! 占号、快照和收尾（`claim_run`、`Lifecycle`）。
//!
//! 任务记录（runs 表，多记一个 `engine`）、一个项目一次一个任务、影子仓库的快照（改动记录和按快照回退）、
//! `run_cancel` 能停。任务事件（`RunEvent`）只发开始、结束这几条进任务记录；过程是统一事件（`AgentEvent`），
//! 经 `channel` 发给界面、存进事件日志。
//!
//! 审批和提问的回答：IPC `gateway_control(runId, control)`。Grok 跑的时候顺手开 Peck 的
//! 本机 MCP（`ask_user`）。用非默认账号跑时，引擎报的额度不写进顶栏 QUOTA 的卡片。

use super::event::{AgentEvent, EventBody, PermissionLevel, TurnEndReason};
use super::log::EventSink;
use super::{catalog, engines, mcp, AgentTask, Control};
use crate::events::{RunStatus, StepStatus};
use crate::harness::run::permission_of;
use crate::run::ActiveRunContext;
use crate::state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

fn controls() -> &'static Mutex<HashMap<String, mpsc::UnboundedSender<Control>>> {
    static C: OnceLock<Mutex<HashMap<String, mpsc::UnboundedSender<Control>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
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
    // 独立工作区（v0.7）：在项目的一个 git worktree 里做，做完再搬回。
    worktree: Option<bool>,
    // 这一句附的图（v0.8）：存成文件，路径写在这一句后面。
    images: Option<Vec<super::ImageInput>>,
    // 这一句附的文档和文件夹（v1.1.8，输入框 + 按钮挑的）。
    uploads: Option<Vec<String>>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let level = permission_of(permission.as_deref())?;
    let Some(engine) = who.strip_prefix("cli:").map(str::to_string) else {
        return Err("这个入口只跑 CLI 引擎（cli:…）。".into());
    };
    let label = catalog::catalog()
        .engines
        .iter()
        .find(|e| e.id == engine)
        .map(|e| e.label.clone())
        .ok_or_else(|| format!("不认识的引擎 {engine}。"))?;
    let backend = engines::from_state(&state, &engine).await.map_err(|e| e.message)?;
    let images = crate::harness::run::checked_images(images)?;
    let attachments = crate::harness::run::save_attachments(&session_id, &images);
    let saved_uploads = crate::harness::run::save_uploads(&session_id, &uploads.unwrap_or_default())?;
    let prompt = format!("{prompt}{}", crate::harness::run::uploads_note(&saved_uploads));
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
            worktree: worktree.unwrap_or(false),
            attachments,
            compact: false,
        },
        backend,
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

/// `/compact [重点]` 转给 CLI（v1.1.7）：续上这个任务里它的会话，让它自己压一次（Codex、Muse 用专用的请求，
/// Claude Code、Grok 发文字命令）。落成这个任务的一次新记录。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn gateway_compact(
    session_id: String,
    who: String,
    focus: Option<String>,
    permission: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    resume: Option<String>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let level = permission_of(permission.as_deref())?;
    let Some(engine) = who.strip_prefix("cli:").map(str::to_string) else {
        return Err("这个入口只转给 CLI 引擎（cli:…）。".into());
    };
    let label = catalog::catalog().engines.iter().find(|e| e.id == engine).map(|e| e.label.clone()).ok_or_else(|| format!("不认识的引擎 {engine}。"))?;
    let Some(resume) = resume.filter(|r| !r.trim().is_empty()) else {
        return Err(format!("这个任务还没有 {label} 的会话，没什么可压的。"));
    };
    let backend = engines::from_state(&state, &engine).await.map_err(|e| e.message)?;
    if !backend.caps().resume {
        return Err(format!("{label} 续不了会话，压不了。"));
    }
    let focus = focus.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
    let prompt = match &focus {
        Some(f) => format!("/compact {f}"),
        None => "/compact".into(),
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
            resume: Some(resume),
            worktree: false,
            attachments: vec![],
            compact: true,
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
    /// 在独立工作区（git worktree）里做。
    pub worktree: bool,
    /// 附图存成的文件（v0.8）：路径写在引擎看到的这一句后面。
    pub attachments: Vec<PathBuf>,
    /// 转发 `/compact`（v1.1.7）：续上 `resume`、让 CLI 自己压一次；不带引用、记忆、文件夹说明，Peck 替它跑的 Hook 也不跑。
    pub compact: bool,
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
        worktree,
        attachments,
        compact,
    } = run;
    // 任务记录：占一个号、一个项目一次一个（和 Peck Loop 共用，`run.rs`）。
    let claimed = crate::run::claim_run(crate::run::ClaimSpec {
        store: &store,
        active_runs: &active_runs,
        session_id: &session_id,
        prompt: &prompt,
        who: &who,
        permission: permission.clone(),
        level,
        worktree,
        agent_id: &engine,
        label: &label,
    })?;
    let (run_id, step_id, workspace, workspace_id, cwd) = (claimed.run_id.clone(), claimed.step_id.clone(), claimed.workspace.clone(), claimed.workspace_id.clone(), claimed.cwd.clone());
    let (settings, folders) = (claimed.settings.clone(), claimed.folders.clone());
    let (mut refs, fork, allowed, hints) = {
        let db = store.lock().map_err(|e| crate::run::abandon(&store, &active_runs, &run_id, e.to_string()))?;
        (
            // `@` 进来的旧任务（v0.6）：引擎看到的这一句前面带上那几个任务的经过；任务记录里照旧是站主写的原话。
            crate::harness::history::task_refs(&db, &workspace_id, &prompt),
            // 换了引擎接着发（v0.8）：别家这期间做的带过来，原来那一家的会话留着。
            crate::harness::history::fork_context(&db, &session_id, &format!("cli:{engine}"), &run_id),
            db.harness_allowed(&workspace_id).unwrap_or_default(),
            crate::harness::mcp_client::hints_for(&db, &workspace_id),
        )
    };
    if compact {
        refs.clear();
    } else if let Some(f) = &fork {
        refs.insert(0, f.text.clone());
    }
    let fork_note = fork.as_ref().filter(|_| !compact).map(|f| crate::harness::history::fork_notice(f, &label));
    let engine_prompt = if refs.is_empty() { prompt.clone() } else { format!("{}\n\n{prompt}", refs.join("\n\n")) };
    let engine_prompt = if compact { prompt.clone() } else { format!("{engine_prompt}{}{}", crate::harness::run::attachment_note(&attachments), folders_note(&folders, level)) };
    let project_env = settings.env_pairs();
    // 脚本 Hook（v1.1.4，`cli_hooks.rs`）的「总是允许」和 Peck Loop 共用；引擎的审批规则那一份要交给 sink。
    let hook_allowed = allowed.clone();
    let original_prompt = prompt.clone();
    // 这个任务是谁在干（对话里设定时任务时默认照它）。
    let setup = crate::schedule::Setup { who: who.clone(), model: model.clone(), effort: effort.clone() };

    let (control_tx, control_rx) = mpsc::unbounded_channel();
    if let Ok(mut c) = controls().lock() {
        c.insert(run_id.clone(), control_tx.clone());
    }
    let id = run_id.clone();
    let rule_tx = control_tx.clone();
    let rule_root = cwd.clone();
    tokio::spawn(async move {
        let last_text = Arc::new(Mutex::new(String::new()));
        let seen = last_text.clone();
        // 引擎在收尾那一条里写的原因（「订阅额度用完了，几点重置」这类）：停下时的总结用它，不只写 Error。
        let last_error: Arc<Mutex<Option<String>>> = Arc::default();
        let seen_error = last_error.clone();
        let sink = EventSink::new(id.clone(), Some(step_id.clone()), engine.clone(), Some(store.clone()), move |e: &AgentEvent| {
            if let EventBody::MessageCompleted { text, .. } = &e.body {
                if let Ok(mut t) = seen.lock() {
                    *t = text.clone();
                }
            }
            if let EventBody::TurnCompleted { error: Some(why), .. } = &e.body {
                if let Ok(mut w) = seen_error.lock() {
                    *w = Some(why.clone());
                }
            }
            // Peck 替 CLI 审批（站主 2026-11-16 Phase 13）：CLI 自己的审批比 Peck 的档位严（Codex 的 EDIT 连 cat 都问），
            // 要跑的命令先按 Peck 这一档的规则看，用不着问的直接批掉、不弹卡片；装依赖、起常驻进程、高危的照旧给站主。
            // 委派出去的子任务（v0.9 B1，事件带 subagent_id）是 Peck Loop 自己按档位问的，不替它批、不改卡片。
            // Peck 自己的 Hook 卡片（v1.1.4，`cli_hooks.rs`）原样给站主，不按命令的规则替他批。
            if e.subagent_id.is_none() && !super::cli_hooks::is_hook_card(&e.body) {
                if let Some(request_id) = rule_allows(level, &e.body, &rule_root, &allowed) {
                    let _ = rule_tx.send(Control::ApproveByRule { request_id });
                    return;
                }
                // MCP 工具：Peck 记得它的标注（只读、会改、会删改）就照 Peck Loop 的规矩办。
                match mcp_hint(level, &e.body, &hints, &allowed) {
                    Some(HintAction::Approve(request_id)) => {
                        let _ = rule_tx.send(Control::ApproveByRule { request_id });
                        return;
                    }
                    Some(HintAction::Flag(flagged)) => {
                        out(&AgentEvent { body: flagged, ..e.clone() });
                        return;
                    }
                    None => {}
                }
                // 引擎没标高危、Peck 的规则看是高危的命令（删文件、推送……）：卡片标红、不给「总是允许」，和 Peck Loop 一样。
                if let Some(flagged) = flag_high_risk(&e.body, &rule_root) {
                    out(&AgentEvent { body: flagged, ..e.clone() });
                    return;
                }
            }
            out(e);
        });
        register_sink(&id, &sink);
        if let Some(text) = fork_note {
            sink.emit(EventBody::Notice { level: super::event::NoticeLevel::Info, text });
        }
        let life = crate::run::Lifecycle::begin(&store, &active_runs, &claimed, &engine, &label, level, worktree);
        // 停止：`run_cancel` 改成发打断（本机 MCP 里等着批的回退、提交也一起停）。
        let interrupt = control_tx.clone();
        let stop_id = id.clone();
        let watcher = crate::run::watch_cancel(claimed.cancel_rx.clone(), move || {
            let _ = interrupt.send(Control::Interrupt);
            interrupt_subs(&stop_id);
        });
        // Peck 的本机 MCP（v0.10 起每个 CLI 任务都挂）：`create_skill` 都给（C3），`ask_user` 只给自己没有问站主通道的 Grok。
        let offer = mcp::Offer {
            ask_user: engine == "grok",
            skill_root: Some(workspace.clone()),
            extras: Some(Arc::new(crate::harness::extras::Extras {
                store: store.clone(),
                workspace_id: workspace_id.clone(),
                session_id: session_id.clone(),
                run_id: id.clone(),
                setup: setup.clone(),
            })),
            read_only: level == PermissionLevel::Plan,
            off: crate::plugins::tools_off(&settings.plugins_off),
            git: mcp::GitOffer::of(&workspace, &cwd, level),
        };
        let peck_mcp = mcp::hub().await.ok().map(|h| h.register_offer(sink.clone(), offer));
        let token = peck_mcp.as_ref().map(|m| m.token.clone());
        // 信任（v1.1.8，`trust.rs`）：这家 CLI 没信任这个文件夹、站主也还没在 Peck 里批过——先弹 TRUST 卡片，点了才起 CLI。
        // 批过的每次带上这家官方的开关；NOT NOW 这次打开 Peck 期间不再问。转发 `/compact` 不问。
        let mut control_rx = control_rx;
        let mut held: Option<RunOutcomeOrStop> = None;
        let mut trusted = settings.trusted;
        let mut asked = false;
        if let Some(setup) = backend.setup() {
            if !trusted && !compact && !super::trust::is_skipped(&workspace_id, &engine) && super::trust::untrusted(&engine, setup, &cwd).await {
                asked = true;
                match ask_trust(&sink, &mut control_rx, &control_tx, &id, &engine, &label).await {
                    Some(true) => {
                        trusted = true;
                        if let Ok(db) = store.lock() {
                            if let Ok(mut ws) = db.workspace(&workspace_id) {
                                ws.settings.trusted = true;
                                let _ = db.save_workspace(&ws);
                            }
                        }
                    }
                    Some(false) => super::trust::skip(&workspace_id, &engine),
                    None => held = Some(RunOutcomeOrStop::Interrupted),
                }
            }
            // Grok 没有只这一次的开关：批过、它自己还没记着，就用它的 `--trust` 记一次。
            if trusted && engine == "grok" && held.is_none() && super::trust::untrusted(&engine, setup, &cwd).await {
                if let Err(why) = super::trust::grant_grok(setup, &cwd).await {
                    sink.emit(EventBody::Notice { level: super::event::NoticeLevel::Warn, text: why });
                }
            }
            // A1：项目里配了 MCP、这家 CLI 却没读到——开头写一行。信任的事卡片管了，问过、批过的不再写。
            if !trusted && !asked {
                if let Some(text) = super::project_mcp::check(&engine, &label, setup, &cwd).await {
                    sink.emit(EventBody::Notice { level: super::event::NoticeLevel::Warn, text });
                }
            }
        }
        // 脚本 Hook（v1.1.4）：Peck 替 CLI 跑开场时、发话时（这一句发出去之前，stdout 放在这句前面）和收尾时。
        // 有要跑的，控制通道先交给 `CliHooks`：Hook 卡片的回答归 Peck，别的转给正在跑的那一次引擎。
        let mut control_rx = Some(control_rx);
        let mut hooks = super::cli_hooks::hooks_for(&engine, &cwd).filter(|_| !compact).map(|set| {
            let (remember_store, ws) = (store.clone(), workspace_id.clone());
            super::cli_hooks::CliHooks::new(
                set,
                control_rx.take().expect("控制通道还在"),
                sink.clone(),
                &cwd,
                crate::harness::run::with_project_env(crate::harness::tools::bash::shell_env(), &project_env),
                level,
                format!("peck-{session_id}"),
                hook_allowed,
                Arc::new(move |rule: &str| {
                    if let Ok(db) = remember_store.lock() {
                        let _ = db.harness_allow(&ws, rule);
                    }
                }),
            )
        });
        let mut engine_prompt = engine_prompt;
        // 项目记忆（v1.1.5）：新会话（不是续上的）的第一句前面带上，排在开场时的 Hook 前面。
        let mut contexts: Vec<String> = Vec::new();
        if resume.is_none() && !compact {
            contexts.extend(crate::harness::context::memory_block(&crate::harness::run::memories_of(&store, &workspace_id)));
        }
        if let Some(h) = hooks.as_mut().filter(|_| held.is_none()) {
            use crate::harness::script_hooks::{context_block, Event};
            if resume.is_none() {
                let input = serde_json::json!({"source": "startup", "model": model});
                if let (_, Some(c)) = h.run(Event::SessionStart, Some("startup"), input).await {
                    contexts.push(context_block(Event::SessionStart, &c));
                }
            }
            let (blocked, context) = h.run(Event::UserPromptSubmit, None, serde_json::json!({"prompt": original_prompt})).await;
            contexts.extend(context.map(|c| context_block(Event::UserPromptSubmit, &c)));
            if h.interrupted() {
                held = Some(RunOutcomeOrStop::Interrupted);
            } else if let Some(why) = blocked {
                held = Some(RunOutcomeOrStop::Blocked(format!("项目的 Hook 拦下了这句话：{why}")));
            }
        }
        if !contexts.is_empty() {
            engine_prompt = format!("{}\n\n{engine_prompt}", contexts.join("\n\n"));
        }
        // 控制通道交给引擎（有 Hook 时是 `CliHooks` 转过去的那一条）；按停在引擎起来之前也收得到（通道里先放着）。
        let task = AgentTask {
            run_id: id.clone(),
            prompt: engine_prompt,
            cwd: cwd.clone(),
            permission: level,
            model: model.clone(),
            effort: effort.clone(),
            resume,
            peck_mcp: peck_mcp.clone(),
            env: project_env.clone(),
            folders: folders.clone(),
            compact,
            trusted,
        };
        let mut outcome = match held {
            // 发话时的 Hook 拦下了、卡片等着时按了停：这一句不发。
            Some(stop) => {
                let (reason, error) = match stop {
                    RunOutcomeOrStop::Blocked(why) => (TurnEndReason::Limit, Some(why)),
                    RunOutcomeOrStop::Interrupted => (TurnEndReason::Interrupted, None),
                };
                sink.emit(EventBody::TurnCompleted { turn_id: "turn-1".into(), reason, error });
                Ok(super::RunOutcome { reason, session_ref: task.resume.clone() })
            }
            None => {
                let rx = match hooks.as_ref() {
                    Some(h) => h.engine_channel(),
                    None => control_rx.take().expect("控制通道还在"),
                };
                run_engine(backend.as_ref(), task.clone(), &sink, rx).await
            }
        };
        // 收尾时的 Hook（v1.1.4）：CLI 做完了再跑；退出码 2 就续上它的会话把原因发过去，让它接着干（一回合最多 3 次）。
        // 排在下面 Git Plan 的收尾提交之前。
        if let Some(h) = hooks.as_mut() {
            use crate::harness::script_hooks::{stop_feedback, Event, MAX_STOP_BLOCKS};
            h.engine_done();
            let caps = backend.caps();
            while h.stop_blocks < MAX_STOP_BLOCKS && h.has(Event::Stop) && !h.interrupted() {
                let Ok(done) = &outcome else { break };
                if done.reason != TurnEndReason::Done {
                    break;
                }
                let at = id.clone();
                let changed = tokio::task::spawn_blocking(move || crate::snapshot::changed_since_base(&at)).await.ok().flatten().unwrap_or_default();
                let input = serde_json::json!({"stop_hook_active": h.stop_blocks > 0, "peck_changed_files": changed});
                let (blocked, _) = h.run(Event::Stop, None, input).await;
                let Some(why) = blocked else { break };
                h.stop_blocks += 1;
                let Some(session) = done.session_ref.clone().filter(|_| caps.resume) else {
                    sink.emit(EventBody::Notice { level: super::event::NoticeLevel::Warn, text: format!("收尾时的 Hook 拦下了，但 {label} 续不了会话，没能接着干：{why}") });
                    break;
                };
                let next = AgentTask { prompt: stop_feedback(&why), resume: Some(session), ..task.clone() };
                outcome = run_engine(backend.as_ref(), next, &sink, h.engine_channel()).await;
                h.engine_done();
            }
        }
        watcher.abort();
        if let (Some(token), Ok(h)) = (token, mcp::hub().await) {
            h.unregister(&token);
        }
        if let Ok(mut c) = controls().lock() {
            c.remove(&id);
        }
        unregister_sink(&id);

        let (step_status, run_status, failure) = match &outcome {
            Ok(o) => match o.reason {
                TurnEndReason::Done => (StepStatus::Succeeded, RunStatus::Succeeded, None),
                TurnEndReason::Interrupted => (StepStatus::Cancelled, RunStatus::Cancelled, None),
                other => {
                    let why = last_error.lock().ok().and_then(|w| w.clone()).filter(|w| !w.trim().is_empty());
                    (StepStatus::Failed, RunStatus::Failed, Some(why.unwrap_or_else(|| format!("{other:?}"))))
                }
            },
            Err(e) => (StepStatus::Failed, RunStatus::Failed, Some(e.message.clone())),
        };
        // 引擎停了还没回来的委派子任务：先停掉再拍收尾快照。
        interrupt_subs(&id);
        let (round_files, changeset) = life.snapshot_after();
        // 收尾 Hook（v1.1.0）：做完了、这一轮改的文件还没提交，续上这个 CLI 的会话问一行提交说明，Peck 自己提交。
        let stop_hook = if worktree || compact { None } else { crate::project_plugins::stop_hook(&workspace) };
        let ask_task = AgentTask {
            run_id: id.clone(),
            prompt: String::new(),
            cwd: cwd.clone(),
            permission: PermissionLevel::Plan,
            model: model.clone(),
            effort: effort.clone(),
            resume: outcome.as_ref().ok().and_then(|o| o.session_ref.clone()),
            peck_mcp: None,
            env: project_env.clone(),
            folders: vec![],
            compact: false,
            trusted,
        };
        let (asker, ask, usage_to, asked_engine) = (backend.clone(), stop_hook.as_ref().map(|h| h.ask.clone()).unwrap_or_default(), sink.clone(), engine.clone());
        let hooked = crate::harness::hooks::finish(stop_hook.as_ref(), &cwd, level, run_status == RunStatus::Succeeded, worktree, round_files, || async move {
            crate::harness::hooks::ask_cli(asker.as_ref(), &asked_engine, ask_task, &ask, move |body| {
                usage_to.emit(body);
            })
            .await
        })
        .await;
        if let Some(body) = hooked {
            sink.emit(body);
        }
        let text = last_text.lock().map(|t| t.clone()).unwrap_or_default();
        let summary = match (run_status, failure) {
            (RunStatus::Succeeded, _) if !text.trim().is_empty() => text,
            (RunStatus::Succeeded, _) if compact => format!("{label} 压缩了上下文。"),
            (RunStatus::Succeeded, _) => format!("{label} 跑完了。"),
            (RunStatus::Cancelled, _) => "已停止。".into(),
            (_, Some(why)) => format!("{label} 没跑完：{why}"),
            _ => format!("{label} 没跑完。"),
        };
        life.finish(step_status, run_status, summary, changeset);
    });
    Ok(run_id)
}

/// TRUST 卡片（v1.1.8）：发出去、等站主点。`Some(true)` 批了，`Some(false)` NOT NOW，`None` 等着时按了停。
/// 等的时候来的别的控制（追加的话）先收着，问完放回通道。
async fn ask_trust(
    sink: &EventSink,
    rx: &mut mpsc::UnboundedReceiver<Control>,
    tx: &mpsc::UnboundedSender<Control>,
    run_id: &str,
    engine: &str,
    label: &str,
) -> Option<bool> {
    let request_id = super::trust::request_id(run_id);
    sink.emit(EventBody::TrustPrompt { request_id: request_id.clone(), engine: engine.to_string(), label: label.to_string() });
    let mut stray = Vec::new();
    let answer = loop {
        match rx.recv().await {
            Some(Control::Approve { request_id: r, decision }) if r == request_id => break Some(decision.allows()),
            Some(Control::Interrupt) | None => break None,
            Some(other) => stray.push(other),
        }
    };
    for c in stray {
        let _ = tx.send(c);
    }
    if let Some(trusted) = answer {
        sink.emit(EventBody::TrustResolved { request_id, trusted });
    }
    answer
}

/// 发话时的 Hook 没让这一句发出去：拦下了（原因），或者卡片等着时站主按了停。
enum RunOutcomeOrStop {
    Blocked(String),
    Interrupted,
}

/// 跑一次引擎。起都没起来（找不到程序、起不了进程）时一条事件都没有：补一条出错的收尾，
/// 界面和别的结束一样收掉「在跑」，不然这个项目一直占着。
async fn run_engine(backend: &dyn super::AgentBackend, task: AgentTask, sink: &EventSink, control: mpsc::UnboundedReceiver<Control>) -> Result<super::RunOutcome, super::GatewayError> {
    let outcome = backend.run(task, sink.clone(), control).await;
    if let Err(e) = &outcome {
        sink.emit(EventBody::Error { message: e.message.clone(), fatal: true });
        sink.emit(EventBody::TurnCompleted { turn_id: "turn-1".into(), reason: TurnEndReason::Error, error: Some(e.message.clone()) });
    }
    outcome
}

/// 项目的其他文件夹（v0.10）：写在这一句后面，告诉 CLI 能读哪些、能改哪些（PLAN 一律只读）。
pub(crate) fn folders_note(folders: &[(PathBuf, bool)], level: PermissionLevel) -> String {
    if folders.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = folders
        .iter()
        .map(|(p, w)| format!("- {} ({})", p.display(), if *w && level != PermissionLevel::Plan { "you may edit files here" } else { "read-only: do not change files here" }))
        .collect();
    format!("\n\n<peck-folders>\nThis project also uses these folders outside the working directory (absolute paths):\n{}\n</peck-folders>", lines.join("\n"))
}

/// 界面 → 引擎：审批、回答、追加、打断。任务已经结束就报错。
/// CLI 发来的一条审批能不能按 Peck 的规则直接批：EDIT、AUTO 档里要跑的命令，Peck Loop 这一档也不会问的。
/// 能批给回 request_id。PLAN 不替它批（CLI 自己的只读模式说了算）；改文件、MCP 这些也照旧给站主。
pub(crate) fn rule_allows(level: PermissionLevel, body: &EventBody, root: &std::path::Path, allowed: &std::collections::HashSet<String>) -> Option<String> {
    use super::event::ToolKind;
    use crate::harness::permission::{check_command, Ask};
    let EventBody::ApprovalRequested { request_id, kind, title, detail, high_risk: false, options, .. } = body else {
        return None;
    };
    if !options.contains(&super::event::ApprovalDecision::AllowOnce) {
        return None;
    }
    // Peck 自己的本机 MCP（`ask_user`；`create_skill`，v0.10 C3）：CLI 问要不要用，直接批。`create_skill` 只是提议、不写盘，PLAN 也批。
    let peck_tool = *kind == ToolKind::Mcp && title.contains("MCP peck · ");
    // 定时任务那三个不碰项目文件，PLAN 也批；`restore_snapshot` 在 PLAN 下给站主（它自己也会拒掉回退）。
    if peck_tool && ["create_skill", "load_skill", "schedule_task", "list_schedules", "cancel_schedule"].iter().any(|t| title.contains(t)) {
        return Some(request_id.clone());
    }
    if level == PermissionLevel::Plan {
        return None;
    }
    if peck_tool {
        return Some(request_id.clone());
    }
    let Some(detail) = detail.as_ref().filter(|d| !d.trim().is_empty()) else {
        return None;
    };
    let ok = match kind {
        ToolKind::Run => matches!(check_command(level, detail, root, allowed), Ask::No),
        // 改文件：EDIT、AUTO 本来就不问改项目里的；临时目录 Peck Loop 的沙箱也放开。别处、.git 里的照旧给站主。
        // 一行一个文件（Codex 一次改几个文件时）：每个都在项目里才批。
        ToolKind::Edit => detail.lines().filter(|l| !l.trim().is_empty()).all(|l| edit_in_bounds(std::path::Path::new(l.trim()), root)),
        _ => false,
    };
    ok.then(|| request_id.clone())
}

/// 记下的 MCP 工具标注：服务器 → 工具 → 只读 / 会改 / 会删改（`mcp_client::hints_for`）。
pub(crate) type McpHints = HashMap<String, std::collections::BTreeMap<String, crate::harness::mcp_client::ToolHint>>;

/// 照标注怎么处理一张 MCP 审批卡片。
#[derive(Debug, PartialEq)]
pub(crate) enum HintAction {
    /// 替 CLI 批掉（只读的；会改的在 AUTO、或 EDIT 下这台总是允许过）。
    Approve(String),
    /// 会删改的、引擎没标高危：给界面的那一份标红、只留批准一次和不行。
    Flag(EventBody),
}

/// CLI（主要是 Grok：ACP 不带工具标注，每调一次都问）问要不要调 MCP 工具：Peck 起过、TEST 过这台服务器、
/// 记得这个工具的标注，就照 Peck Loop 的规矩办（v0.10，roadmap A2）。认不得的照旧给站主。
/// 卡片标题是转换器写的「MCP <服务器> · <工具>」。
pub(crate) fn mcp_hint(level: PermissionLevel, body: &EventBody, hints: &McpHints, allowed: &std::collections::HashSet<String>) -> Option<HintAction> {
    use super::event::{ApprovalDecision, ToolKind};
    use crate::harness::mcp_client::ToolHint;
    let EventBody::ApprovalRequested { request_id, call_id, kind: ToolKind::Mcp, title, detail, high_risk, options } = body else {
        return None;
    };
    let (server, tool) = title.strip_prefix("MCP ")?.split_once(" · ")?;
    let hint = *hints.get(server)?.get(tool)?;
    match hint {
        ToolHint::Destructive if !*high_risk => Some(HintAction::Flag(EventBody::ApprovalRequested {
            request_id: request_id.clone(),
            call_id: call_id.clone(),
            kind: ToolKind::Mcp,
            title: format!("高危：{title}（会删改东西）"),
            detail: detail.clone(),
            high_risk: true,
            options: options.iter().copied().filter(|o| matches!(o, ApprovalDecision::AllowOnce | ApprovalDecision::DenyOnce)).collect(),
        })),
        _ if *high_risk || !options.contains(&ApprovalDecision::AllowOnce) => None,
        ToolHint::Read => Some(HintAction::Approve(request_id.clone())),
        ToolHint::Write if level == PermissionLevel::Auto || (level == PermissionLevel::Edit && allowed.contains(&format!("mcp:{server}"))) => {
            Some(HintAction::Approve(request_id.clone()))
        }
        _ => None,
    }
}

/// CLI 要跑的命令按 Peck 的规则是高危、引擎自己没标的：给界面的那一份标成高危，选项只留批准一次、不行。
pub(crate) fn flag_high_risk(body: &EventBody, root: &std::path::Path) -> Option<EventBody> {
    use super::event::{ApprovalDecision, ToolKind};
    let EventBody::ApprovalRequested { request_id, call_id, kind: ToolKind::Run, title, detail: Some(command), high_risk: false, options } = body else {
        return None;
    };
    if crate::harness::risk::assess(command, root).is_empty() {
        return None;
    }
    Some(EventBody::ApprovalRequested {
        request_id: request_id.clone(),
        call_id: call_id.clone(),
        kind: ToolKind::Run,
        title: title.clone(),
        detail: Some(command.clone()),
        high_risk: true,
        options: options.iter().copied().filter(|o| matches!(o, ApprovalDecision::AllowOnce | ApprovalDecision::DenyOnce)).collect(),
    })
}

/// 改的文件在项目里（不在 .git 里）或者临时目录里。只认绝对路径，带 `..` 的不认。
fn edit_in_bounds(path: &std::path::Path, root: &std::path::Path) -> bool {
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    if path.components().any(|c| c.as_os_str() == ".git") {
        return false;
    }
    let temp = std::env::temp_dir();
    let roots = [root.to_path_buf(), temp.clone(), std::path::PathBuf::from("/tmp"), std::path::PathBuf::from("/private/tmp")];
    roots.iter().any(|r| path.starts_with(r)) || path.starts_with(temp.canonicalize().unwrap_or(temp))
}

#[tauri::command]
pub async fn gateway_control(run_id: String, control: Control) -> Result<(), String> {
    // 任务跑着时站主追加的一句（v1.1.9）：交给引擎，再记进过程（对话里画在当时那一处，重开还在）。
    if let Control::Append { text } = &control {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("先写一句话。".into());
        }
        let sink = sinks().lock().ok().and_then(|s| s.get(&run_id).cloned());
        route_control(&run_id, Control::Append { text: text.clone() })?;
        if let Some(sink) = sink {
            sink.emit(EventBody::UserAppended { text });
        }
        return Ok(());
    }
    route_control(&run_id, control)
}

/// 在跑的任务的事件出口（追加的话记进过程用）：任务 id → sink。
fn sinks() -> &'static Mutex<HashMap<String, EventSink>> {
    static S: OnceLock<Mutex<HashMap<String, EventSink>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn register_sink(run_id: &str, sink: &EventSink) {
    if let Ok(mut s) = sinks().lock() {
        s.insert(run_id.to_string(), sink.clone());
    }
}

pub(crate) fn unregister_sink(run_id: &str) {
    if let Ok(mut s) = sinks().lock() {
        s.remove(run_id);
    }
}

/// 本机 MCP 里等着站主批的事（回退、提交走的子通道）：任务 id → [(审批、提问编号的前缀, 通道)]。
fn sub_controls() -> &'static Mutex<HashMap<String, Vec<(String, mpsc::UnboundedSender<Control>)>>> {
    static C: OnceLock<Mutex<HashMap<String, Vec<(String, mpsc::UnboundedSender<Control>)>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 子任务跑起来：它的审批、提问编号都以 `prefix` 开头，站主的回答按前缀转给它；打断两边都发。
pub(crate) fn register_sub_control(run_id: &str, prefix: &str, tx: mpsc::UnboundedSender<Control>) {
    if let Ok(mut c) = sub_controls().lock() {
        c.entry(run_id.to_string()).or_default().push((prefix.to_string(), tx));
    }
}

/// 按 STOP、任务收尾：这个任务还在跑的委派子任务都打断（以前 STOP 只停 CLI，子任务在本机 MCP 里接着改文件，
/// 改动不进这次的记录、回退也放不回去；v0.10 收尾审查）。
pub(crate) fn interrupt_subs(run_id: &str) {
    let subs: Vec<mpsc::UnboundedSender<Control>> = sub_controls().lock().ok().and_then(|c| c.get(run_id).map(|l| l.iter().map(|(_, tx)| tx.clone()).collect())).unwrap_or_default();
    for tx in subs {
        let _ = tx.send(Control::Interrupt);
    }
}

/// member 收尾时还有它派的 explorer、searcher 在跑（v1.1.3）：只叫停编号在它下面的那几个（不碰别的子代理）。
pub(crate) fn interrupt_subs_under(run_id: &str, prefix: &str) {
    let subs: Vec<mpsc::UnboundedSender<Control>> = sub_controls()
        .lock()
        .ok()
        .and_then(|c| c.get(run_id).map(|l| l.iter().filter(|(p, _)| p != prefix && p.starts_with(prefix)).map(|(_, tx)| tx.clone()).collect()))
        .unwrap_or_default();
    for tx in subs {
        let _ = tx.send(Control::Interrupt);
    }
}

pub(crate) fn unregister_sub_control(run_id: &str, prefix: &str) {
    if let Ok(mut c) = sub_controls().lock() {
        if let Some(list) = c.get_mut(run_id) {
            list.retain(|(p, _)| p != prefix);
            if list.is_empty() {
                c.remove(run_id);
            }
        }
    }
}

/// 审批、回答、追加、打断送到该去的地方：编号是子任务的给子任务，其余给任务本身（打断也告诉子任务）。
/// 界面的 `gateway_control` 和 Peck 自己替 CLI 批（规则批的）都走这里。
pub(crate) fn route_control(run_id: &str, control: Control) -> Result<(), String> {
    let request_id = match &control {
        Control::Approve { request_id, .. } | Control::ApproveByRule { request_id } | Control::Answer { request_id, .. } => Some(request_id.clone()),
        _ => None,
    };
    let subs: Vec<(String, mpsc::UnboundedSender<Control>)> = sub_controls().lock().ok().and_then(|c| c.get(run_id).cloned()).unwrap_or_default();
    if let Some(rid) = &request_id {
        // 最长的前缀：member 派的 explorer 编号是「agent-2-agent-1-…」，不能被 member 自己的「agent-2-」截走（v1.1.3）。
        if let Some((_, tx)) = subs.iter().filter(|(p, _)| rid.starts_with(p.as_str())).max_by_key(|(p, _)| p.len()) {
            return tx.send(control).map_err(|_| "这个子任务已经结束了。".into());
        }
    }
    if control == Control::Interrupt {
        for (_, tx) in &subs {
            let _ = tx.send(Control::Interrupt);
        }
    }
    let tx = controls().lock().ok().and_then(|c| c.get(run_id).cloned());
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
    use crate::flow::DispatchPlan;

    /// member 派的 explorer 编号在 member 的前缀下面（v1.1.3）：审批按最长的前缀转；叫停 member 下面的不碰别人。
    #[test]
    fn nested_subagents_get_their_own_approvals_and_interrupts() {
        let run = "run-nested-route";
        let (member_tx, mut member_rx) = mpsc::unbounded_channel();
        let (explorer_tx, mut explorer_rx) = mpsc::unbounded_channel();
        let (other_tx, mut other_rx) = mpsc::unbounded_channel();
        register_sub_control(run, "agent-2-", member_tx);
        register_sub_control(run, "agent-2-agent-1-", explorer_tx);
        register_sub_control(run, "agent-3-", other_tx);
        let approve = |id: &str| Control::Approve { request_id: id.into(), decision: super::super::event::ApprovalDecision::AllowOnce };
        route_control(run, approve("agent-2-agent-1-approve-1")).unwrap();
        route_control(run, approve("agent-2-approve-1")).unwrap();
        assert!(matches!(explorer_rx.try_recv(), Ok(Control::Approve { request_id, .. }) if request_id == "agent-2-agent-1-approve-1"));
        assert!(matches!(member_rx.try_recv(), Ok(Control::Approve { request_id, .. }) if request_id == "agent-2-approve-1"));
        interrupt_subs_under(run, "agent-2-");
        assert_eq!(explorer_rx.try_recv().ok(), Some(Control::Interrupt));
        assert!(member_rx.try_recv().is_err() && other_rx.try_recv().is_err(), "只叫停它下面的");
        for prefix in ["agent-2-", "agent-2-agent-1-", "agent-3-"] {
            unregister_sub_control(run, prefix);
        }
    }

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
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        // 快照登记表是整个进程共用的，别的测试会用 run-105：先占一个大号，这个任务拿到 run-910002。
        let mut store = store;
        let filler = DispatchPlan { steps: vec![] };
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
            worktree: false,
            attachments: vec![],
            compact: false,
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
            worktree: false,
            attachments: vec![],
            compact: false,
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
        // 录下来的三次审批里，普通命令 Peck 按 EDIT 的规则替站主批了，到界面上的少于三次。
        assert!(approvals < 3, "{approvals}");
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
        assert_eq!(logged.iter().filter(|e| matches!(e.body, EventBody::ApprovalResolved { .. })).count(), 3, "三次都批了");
        drop(db);
        assert!(gateway_control(run_id, Control::Interrupt).await.is_err(), "结束了就收不到控制");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 假 CLI（v1.1.4 的 Hook 测试用）：记下每次收到的任务；第一次在项目里写一个文件。
    struct HookedCli {
        seen: Arc<Mutex<Vec<AgentTask>>>,
    }

    impl super::super::AgentBackend for HookedCli {
        fn caps(&self) -> super::super::caps::AgentCaps {
            super::super::caps::AgentCaps { resume: true, ..super::super::caps::AgentCaps::peck_loop() }
        }

        fn run(&self, task: AgentTask, sink: EventSink, _control: mpsc::UnboundedReceiver<Control>) -> super::super::BoxFuture<Result<super::super::RunOutcome, super::super::GatewayError>> {
            let n = {
                let mut seen = self.seen.lock().unwrap();
                seen.push(task.clone());
                seen.len()
            };
            Box::pin(async move {
                if n == 1 {
                    std::fs::write(task.cwd.join("a.txt"), "改了\n").unwrap();
                }
                sink.emit(EventBody::MessageCompleted { item_id: format!("m{n}"), text: format!("第 {n} 次做完了") });
                sink.emit(EventBody::TurnCompleted { turn_id: format!("turn-{n}"), reason: TurnEndReason::Done, error: None });
                Ok(super::super::RunOutcome { reason: TurnEndReason::Done, session_ref: Some(format!("s{n}")) })
            })
        }
    }

    /// CLI 的脚本 Hook（v1.1.4）和项目记忆（v1.1.5）：新会话先带记忆；开场时、发话时的 stdout 放在这一句前面；收尾时拦下就续上它的会话、把原因发过去，
    /// 转发 `/compact`（v1.1.7）：续上会话，引擎拿到的就是 `/compact 重点`（不带记忆、引用、文件夹说明），Peck 替它跑的 Hook 不跑。
    #[tokio::test]
    async fn compact_goes_to_the_cli_as_is_without_hooks_or_memory() {
        let root = std::env::temp_dir().join(format!("peck-runner-compact-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(ws_dir.join(".claude")).unwrap();
        let settings = serde_json::json!({"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "echo 发话补充"}]}]}});
        std::fs::write(ws_dir.join(".claude/settings.json"), settings.to_string()).unwrap();
        let store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace { id: "w1".into(), path: ws_dir.to_string_lossy().into_owned(), name: "试".into(), last_opened_at: 0, git_branch: None, hidden: false, settings: Default::default() };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        store.add_memory(&ws.id, "测试用 pnpm test", None).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>> = Arc::default();
        let seen: Arc<Mutex<Vec<AgentTask>>> = Arc::default();
        let backend: Arc<dyn super::super::AgentBackend> = Arc::new(HookedCli { seen: seen.clone() });
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run = EngineRun {
            store: store.clone(),
            active_runs: active_runs.clone(),
            session_id: session.id.clone(),
            prompt: "/compact 留住接口那段".into(),
            who: "cli:codex".into(),
            permission: Some("edit".into()),
            level: PermissionLevel::Edit,
            engine: "codex".into(),
            label: "Codex".into(),
            model: None,
            effort: None,
            resume: Some("s0".into()),
            worktree: false,
            attachments: vec![],
            compact: true,
        };
        start_engine_run(run, backend, move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut cards = 0;
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Some(e)) => cards += matches!(e.body, EventBody::ApprovalRequested { .. }) as u32,
                _ => break,
            }
            if active_runs.lock().unwrap().is_empty() {
                break;
            }
        }
        for _ in 0..200 {
            if active_runs.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(cards, 0, "Peck 替 CLI 跑的 Hook 不跑");
        let tasks = seen.lock().unwrap().clone();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].compact && tasks[0].prompt == "/compact 留住接口那段" && tasks[0].resume.as_deref() == Some("s0"), "{:?}", tasks[0].prompt);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 脚本拿得到这一轮改了的文件；每条命令问一次。
    #[tokio::test]
    async fn cli_hooks_wrap_the_sentence_and_keep_the_session_going() {
        let root = std::env::temp_dir().join(format!("peck-runner-hooks-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(ws_dir.join(".claude")).unwrap();
        let stop = format!(
            "cat > {input}; if [ -f {flag} ]; then exit 0; fi; touch {flag}; echo 先补上测试 >&2; exit 2",
            input = root.join("stop-input.json").display(),
            flag = root.join("done.flag").display()
        );
        let settings = serde_json::json!({"hooks": {
            "SessionStart": [{"matcher": "startup", "hooks": [{"type": "command", "command": "echo 开场补充"}]}],
            "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "echo 发话补充"}]}],
            "Stop": [{"hooks": [{"type": "command", "command": stop}]}]
        }});
        std::fs::write(ws_dir.join(".claude/settings.json"), settings.to_string()).unwrap();
        let store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace { id: "w1".into(), path: ws_dir.to_string_lossy().into_owned(), name: "试".into(), last_opened_at: 0, git_branch: None, hidden: false, settings: Default::default() };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        store.add_memory(&ws.id, "测试用 pnpm test", None).unwrap();
        let mut store = store;
        store.create_run(&session.id, "占号", 970_001, &DispatchPlan { steps: vec![] }).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>> = Arc::default();
        let seen: Arc<Mutex<Vec<AgentTask>>> = Arc::default();
        let backend: Arc<dyn super::super::AgentBackend> = Arc::new(HookedCli { seen: seen.clone() });
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run = EngineRun {
            store: store.clone(),
            active_runs: active_runs.clone(),
            session_id: session.id.clone(),
            prompt: "改一下".into(),
            who: "cli:codex".into(),
            permission: Some("auto".into()),
            level: PermissionLevel::Auto,
            engine: "codex".into(),
            label: "Codex".into(),
            model: None,
            effort: None,
            resume: None,
            worktree: false,
            attachments: vec![],
            compact: false,
        };
        let run_id = start_engine_run(run, backend, move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        let mut cards = 0;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        while !active_runs.lock().unwrap().is_empty() || cards == 0 {
            let Ok(Some(e)) = tokio::time::timeout_at(deadline, rx.recv()).await else { break };
            if let EventBody::ApprovalRequested { request_id, .. } = &e.body {
                assert!(request_id.starts_with("hook-approve-"));
                cards += 1;
                gateway_control(run_id.clone(), Control::Approve { request_id: request_id.clone(), decision: super::super::event::ApprovalDecision::AllowOnce }).await.unwrap();
            }
        }
        for _ in 0..200 {
            if active_runs.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(cards, 3, "三条命令各问一次，收尾那条第二次不再问");
        let tasks = seen.lock().unwrap().clone();
        assert_eq!(tasks.len(), 2, "收尾拦了一次：续上会话接着干");
        let first = &tasks[0].prompt;
        assert!(first.starts_with("<peck-memory>\nNotes about this project") && first.contains("- 测试用 pnpm test\n</peck-memory>"), "新会话先带项目记忆：{first}");
        assert!(first.find("</peck-memory>").unwrap() < first.find("<hook-context event=\"SessionStart\">\n开场补充").unwrap(), "记忆在开场时的 Hook 前面");
        assert!(first.find("发话补充").unwrap() < first.find("改一下").unwrap());
        assert_eq!(tasks[1].resume.as_deref(), Some("s1"));
        assert!(tasks[1].prompt.contains("先补上测试") && tasks[1].prompt.contains("<hook-feedback event=\"Stop\">"));
        assert!(!tasks[1].prompt.contains("peck-memory"), "续上的会话不再带");
        let input = std::fs::read_to_string(root.join("stop-input.json")).unwrap();
        assert!(input.contains("\"peck_changed_files\":[\"a.txt\"]"), "{input}");
        let db = store.lock().unwrap();
        let record = db.runs(&session.id).unwrap().into_iter().find(|r| r.id == run_id).unwrap();
        assert_eq!(record.status, RunStatus::Succeeded);
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn other_folders_are_told_to_the_cli() {
        let folders = vec![(PathBuf::from("/lib/shared"), true), (PathBuf::from("/docs"), false)];
        let note = folders_note(&folders, PermissionLevel::Edit);
        assert!(note.contains("/lib/shared (you may edit files here)") && note.contains("/docs (read-only"));
        assert!(folders_note(&folders, PermissionLevel::Plan).matches("read-only").count() == 2);
        assert_eq!(folders_note(&[], PermissionLevel::Edit), "");
    }

    #[test]
    fn peck_approves_plain_cli_commands_by_its_own_rules() {
        use super::super::event::{ApprovalDecision, ToolKind};
        let ask = |kind: ToolKind, cmd: &str, high_risk: bool| EventBody::ApprovalRequested {
            request_id: "r1".into(),
            call_id: None,
            kind,
            title: format!("跑 {cmd}"),
            detail: Some(cmd.into()),
            high_risk,
            options: vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce],
        };
        let root = std::path::Path::new("/p");
        let none = std::collections::HashSet::new();
        let edit = PermissionLevel::Edit;
        // 读、查、构建、测试：Peck 的 EDIT 不问，替 CLI 批掉。
        for cmd in ["/bin/zsh -lc 'cat CLAUDE.md ROADMAP.md'", "git status --short && git branch --show-current", "rg -n peck src", "pnpm test"] {
            assert_eq!(rule_allows(edit, &ask(ToolKind::Run, cmd, false), root, &none).as_deref(), Some("r1"), "{cmd}");
        }
        // 装依赖、起常驻进程：EDIT 照旧问；「总是允许」过的就不问；AUTO 不问。
        assert!(rule_allows(edit, &ask(ToolKind::Run, "/bin/zsh -lc 'pnpm install'", false), root, &none).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Run, "pnpm dev", false), root, &none).is_none());
        let deps: std::collections::HashSet<String> = ["install-deps".to_string()].into();
        assert!(rule_allows(edit, &ask(ToolKind::Run, "pnpm install", false), root, &deps).is_some());
        assert!(rule_allows(PermissionLevel::Auto, &ask(ToolKind::Run, "pnpm dev", false), root, &none).is_some());
        // 高危、引擎自己标了高危、PLAN、不是命令的：都给站主。
        assert!(rule_allows(edit, &ask(ToolKind::Run, "rm -rf build", false), root, &none).is_none());
        assert!(rule_allows(PermissionLevel::Auto, &ask(ToolKind::Run, "git push", false), root, &none).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Run, "cat a", true), root, &none).is_none());
        assert!(rule_allows(PermissionLevel::Plan, &ask(ToolKind::Run, "cat a", false), root, &none).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Edit, "a.txt", false), root, &none).is_none());
        // 改文件：项目里、临时目录里的替它批；项目外、.git 里、带 .. 的、PLAN 的不批。
        assert!(rule_allows(edit, &ask(ToolKind::Edit, "/p/src/a.ts", false), root, &none).is_some());
        assert!(rule_allows(PermissionLevel::Auto, &ask(ToolKind::Edit, "/tmp/grok-icons.html", false), root, &none).is_some());
        assert!(rule_allows(edit, &ask(ToolKind::Edit, "/Users/x/.zshrc", false), root, &none).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Edit, "/p/.git/config", false), root, &none).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Edit, "/p/../etc/hosts", false), root, &none).is_none());
        assert!(rule_allows(PermissionLevel::Plan, &ask(ToolKind::Edit, "/p/a.ts", false), root, &none).is_none());
        // 高危的命令：界面上那一份标红、只给批准一次和不行。
        match flag_high_risk(&ask(ToolKind::Run, "/bin/zsh -lc 'rm -f /tmp/a.html'", false), root) {
            Some(EventBody::ApprovalRequested { high_risk: true, options, .. }) => assert_eq!(options, vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce]),
            other => panic!("{other:?}"),
        }
        assert!(flag_high_risk(&ask(ToolKind::Run, "cat a", false), root).is_none());
        assert!(rule_allows(edit, &ask(ToolKind::Mcp, "dw.write_board", false), root, &none).is_none());
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
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        let plan = DispatchPlan {
            steps: vec![],
        };
        store.create_run(&session.id, "x", 1, &plan).unwrap();
        assert_eq!(store.run_engine("run-1").unwrap(), None, "旧任务没有引擎");
        store.set_run_engine("run-1", "codex").unwrap();
        assert_eq!(store.run_engine("run-1").unwrap().as_deref(), Some("codex"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

// ── Phase 6 追加：Peck Loop（`api:…`）和 ONE SHOT 的任务也登记在这里，界面照旧用 `gateway_control` ──

/// 登记一个任务的控制通道（`harness/run.rs` 起任务时用）。
pub(crate) fn register_control(run_id: &str, tx: mpsc::UnboundedSender<Control>) {
    if let Ok(mut c) = controls().lock() {
        c.insert(run_id.to_string(), tx);
    }
}

/// 任务结束：拿掉它的控制通道。
pub(crate) fn unregister_control(run_id: &str) {
    if let Ok(mut c) = controls().lock() {
        c.remove(run_id);
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    /// 任务跑着时追加一句（v1.1.9）：交给引擎，也记进过程；空话不收，任务结束了报错。
    #[tokio::test]
    async fn an_appended_sentence_reaches_the_engine_and_the_log() {
        let (sink, events) = EventSink::memory("codex");
        let (tx, mut rx) = mpsc::unbounded_channel();
        register_control("run-append", tx);
        register_sink("run-append", &sink);
        gateway_control("run-append".into(), Control::Append { text: "  顺便把 README 也改了  ".into() }).await.unwrap();
        assert_eq!(rx.recv().await, Some(Control::Append { text: "顺便把 README 也改了".into() }));
        assert!(matches!(&events.lock().unwrap()[0].body, EventBody::UserAppended { text } if text == "顺便把 README 也改了"));
        assert!(gateway_control("run-append".into(), Control::Append { text: "  ".into() }).await.is_err());
        unregister_control("run-append");
        unregister_sink("run-append");
        assert!(gateway_control("run-append".into(), Control::Append { text: "晚了".into() }).await.is_err());
        assert_eq!(events.lock().unwrap().len(), 1, "没送到的不记");
    }

    /// TRUST 卡片（v1.1.8）：发出去、等站主点；等的时候来的追加先收着、问完放回去；按停就是没答。
    #[tokio::test]
    async fn the_trust_card_waits_for_the_answer_and_keeps_stray_controls() {
        use crate::gateway::event::ApprovalDecision;
        let (sink, events) = EventSink::memory("muse");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let id = super::super::trust::request_id("run-t");
        tx.send(Control::Append { text: "顺便".into() }).unwrap();
        tx.send(Control::Approve { request_id: "别的".into(), decision: ApprovalDecision::AllowOnce }).unwrap();
        tx.send(Control::Approve { request_id: id.clone(), decision: ApprovalDecision::AllowAlways }).unwrap();
        assert_eq!(ask_trust(&sink, &mut rx, &tx, "run-t", "muse", "Muse").await, Some(true));
        assert!(matches!(rx.recv().await, Some(Control::Append { .. })), "追加的话放回通道");
        assert!(matches!(rx.recv().await, Some(Control::Approve { .. })));
        let seen: Vec<&'static str> = events.lock().unwrap().iter().map(|e| e.body.type_name()).collect();
        assert_eq!(seen, ["trust.prompt", "trust.resolved"]);

        tx.send(Control::Approve { request_id: id.clone(), decision: ApprovalDecision::DenyOnce }).unwrap();
        assert_eq!(ask_trust(&sink, &mut rx, &tx, "run-t", "muse", "Muse").await, Some(false));
        tx.send(Control::Interrupt).unwrap();
        assert_eq!(ask_trust(&sink, &mut rx, &tx, "run-t", "muse", "Muse").await, None);
        // 按停的那一次没有「答了」。
        assert_eq!(events.lock().unwrap().iter().filter(|e| e.body.type_name() == "trust.resolved").count(), 2);
    }

    #[test]
    fn answers_for_a_subtask_go_to_it_and_interrupts_reach_both() {
        let (main_tx, mut main_rx) = mpsc::unbounded_channel();
        let (sub_tx, mut sub_rx) = mpsc::unbounded_channel();
        register_control("run-route", main_tx);
        register_sub_control("run-route", "peck-sub-1-", sub_tx);
        let approve = |id: &str| Control::Approve { request_id: id.into(), decision: crate::gateway::event::ApprovalDecision::AllowOnce };
        route_control("run-route", approve("peck-sub-1-approve-1")).unwrap();
        route_control("run-route", approve("call_42")).unwrap();
        route_control("run-route", Control::Interrupt).unwrap();
        assert_eq!(sub_rx.try_recv().unwrap(), approve("peck-sub-1-approve-1"));
        assert_eq!(sub_rx.try_recv().unwrap(), Control::Interrupt);
        assert_eq!(main_rx.try_recv().unwrap(), approve("call_42"));
        assert_eq!(main_rx.try_recv().unwrap(), Control::Interrupt);
        unregister_sub_control("run-route", "peck-sub-1-");
        route_control("run-route", approve("peck-sub-1-approve-2")).unwrap();
        assert_eq!(main_rx.try_recv().unwrap(), approve("peck-sub-1-approve-2"), "子任务结束后都给任务本身");
        unregister_control("run-route");
    }

    #[test]
    fn mcp_approvals_from_acp_follow_the_recorded_tool_hints() {
        use crate::gateway::event::{ApprovalDecision, ToolKind};
        use crate::harness::mcp_client::ToolHint;
        let mut hints: McpHints = HashMap::new();
        hints.insert("dw".into(), [("list".to_string(), ToolHint::Read), ("write_board".to_string(), ToolHint::Write), ("wipe".to_string(), ToolHint::Destructive)].into_iter().collect());
        let ask = |title: &str, options: Vec<ApprovalDecision>| EventBody::ApprovalRequested {
            request_id: "r1".into(),
            call_id: None,
            kind: ToolKind::Mcp,
            title: title.into(),
            detail: Some("{}".into()),
            high_risk: false,
            options,
        };
        let all = vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce];
        let none = std::collections::HashSet::new();
        // 只读的哪一档都批（Grok 的「MCP dw · list」）。
        assert_eq!(mcp_hint(PermissionLevel::Plan, &ask("MCP dw · list", all.clone()), &hints, &none), Some(HintAction::Approve("r1".into())));
        // 会改的：AUTO 批；EDIT 要这台总是允许过；PLAN 不批。
        let write = ask("MCP dw · write_board", vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce]);
        assert_eq!(mcp_hint(PermissionLevel::Auto, &write, &hints, &none), Some(HintAction::Approve("r1".into())));
        assert_eq!(mcp_hint(PermissionLevel::Edit, &write, &hints, &none), None);
        let allowed: std::collections::HashSet<String> = ["mcp:dw".to_string()].into_iter().collect();
        assert_eq!(mcp_hint(PermissionLevel::Edit, &write, &hints, &allowed), Some(HintAction::Approve("r1".into())));
        assert_eq!(mcp_hint(PermissionLevel::Plan, &write, &hints, &allowed), None);
        // 会删改的：标红、只留批准一次和不行，哪一档都给站主。
        match mcp_hint(PermissionLevel::Auto, &ask("MCP dw · wipe", all.clone()), &hints, &allowed) {
            Some(HintAction::Flag(EventBody::ApprovalRequested { high_risk: true, options, title, .. })) => {
                assert_eq!(options, vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce]);
                assert!(title.starts_with("高危："));
            }
            other => panic!("{other:?}"),
        }
        // 认不得的服务器、工具，照旧给站主。
        assert_eq!(mcp_hint(PermissionLevel::Auto, &ask("MCP fs · read", all.clone()), &hints, &none), None);
        assert_eq!(mcp_hint(PermissionLevel::Auto, &ask("MCP dw · new_thing", all), &hints, &none), None);
    }

    #[test]
    fn peck_approves_calls_to_its_own_mcp_tools_only() {
        use crate::gateway::event::{ApprovalDecision, ToolKind};
        let ask = |title: &str| EventBody::ApprovalRequested {
            request_id: "q1".into(),
            call_id: None,
            kind: ToolKind::Mcp,
            title: title.into(),
            detail: None,
            high_risk: false,
            options: vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce],
        };
        let root = std::path::Path::new("/tmp");
        let none = std::collections::HashSet::new();
        assert_eq!(rule_allows(PermissionLevel::Edit, &ask("用 MCP peck · git_commit？"), root, &none).as_deref(), Some("q1"));
        assert_eq!(rule_allows(PermissionLevel::Edit, &ask("MCP peck · ask_user"), root, &none).as_deref(), Some("q1"));
        assert_eq!(rule_allows(PermissionLevel::Edit, &ask("用 MCP dw · write_board？"), root, &none), None, "别的 MCP 照旧问站主");
        assert_eq!(rule_allows(PermissionLevel::Plan, &ask("用 MCP peck · git_commit？"), root, &none), None);
        // Codex 改项目里的文件（一行一个路径）：EDIT 批；有一个在项目外就给站主。
        let edit = |detail: &str| EventBody::ApprovalRequested {
            request_id: "q1".into(),
            call_id: None,
            kind: ToolKind::Edit,
            title: "改 a.py".into(),
            detail: Some(detail.into()),
            high_risk: false,
            options: vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce],
        };
        let proj = std::path::Path::new("/proj");
        assert_eq!(rule_allows(PermissionLevel::Edit, &edit("/proj/a.py\n/proj/b/c.py"), proj, &none).as_deref(), Some("q1"));
        assert_eq!(rule_allows(PermissionLevel::Edit, &edit("/proj/a.py\n/etc/hosts"), proj, &none), None);
        assert_eq!(rule_allows(PermissionLevel::Plan, &edit("/proj/a.py"), proj, &none), None);
        // 提议 Skill 不写盘：PLAN 也批（v0.10 C3）。
        assert_eq!(rule_allows(PermissionLevel::Plan, &ask("MCP peck · create_skill"), root, &none).as_deref(), Some("q1"));
    }
}
