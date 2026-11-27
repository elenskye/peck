//! 评测（v1.3.0）：`peck eval <attempt.json>` 不起窗口跑一次尝试，走界面发任务的同一条路（`start_peck_run`）。
//! 只给 `evals/run.ts` 调（见 `evals/README.md`）；不带参数的正常启动不经过这里。
//!
//! 数据目录照 `PECK_DATA_DIR`、`PECK_SHADOW_ROOT`（调用方每次给新的临时目录）。做的事和站主在界面里一样：
//! 打开项目、点开时认结构和装默认插件（Git Plan）、新建任务、照设置发 PLOOP。没人看着：审批卡片一律拒、ASK 回一句固定的话，
//! 都记次数。结果写进 `out`：`outcome.json`（怎么结束的、计数、花费）、`events.jsonl`（`agent_events` 原样导出）。

use crate::gateway::event::{AgentEvent, ApprovalDecision, EventBody};
use crate::gateway::mock::{ScriptTurn, ScriptedModel};
use crate::gateway::{Control, ModelProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const USAGE: &str = "peck eval <attempt.json>  跑一次评测尝试（evals/run.ts 调用；格式见 evals/README.md）";

/// 一次尝试的参数（`evals/run.ts` 写）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Attempt {
    /// 工作区：调用方建好的 git 仓库。
    workspace: PathBuf,
    /// 给模型的那句话，原样发。
    prompt: String,
    /// `plan` / `edit` / `auto`。
    #[serde(default = "auto")]
    permission: String,
    /// 臂的界面设置，原样存进 `app_settings` 的 `ui`（`ploop.main`、`efforts`、`subagents`、`maxRounds`…）。
    ui: Value,
    /// 假模型的剧本：有就照它答、不调 API（也不派子代理）。
    #[serde(default)]
    script: Option<PathBuf>,
    /// Key 目录（一个供应商一个文件，Google 的叫 `gemini`）：真跑才要。
    #[serde(default)]
    keys: Option<PathBuf>,
    /// 这一次最多花多少 USD（Peck 自己的花费上限，到了就停）。
    #[serde(default)]
    cost_cap: Option<f64>,
    /// 到时候就打断。
    timeout_secs: u64,
    /// ASK 的回答。
    ask_reply: String,
    /// 结果写在这里。
    out: PathBuf,
}

fn auto() -> String {
    "auto".into()
}

/// `outcome.json`。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Outcome {
    run_id: Option<String>,
    /// 认完结构、装完插件以后的那一笔（判分从这里比）。
    base_commit: Option<String>,
    /// `done` / `limit` / `refused` / `interrupted` / `error`，到时间是 `timeout`，没发出去是 `setup`。
    reason: String,
    error: Option<String>,
    model: String,
    effort: Option<String>,
    started_at: i64,
    ended_at: i64,
    approvals_denied: u32,
    high_risk_denied: u32,
    asks_answered: u32,
    cost_usd: f64,
    prepared: Vec<String>,
}

/// `peck eval …`：是就跑完返回退出码，不是返回 `None`（照常起 App）。
pub fn from_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("eval") {
        return None;
    }
    let path = match args.next() {
        Some(p) if p != "--help" && p != "-h" => PathBuf::from(p),
        _ => {
            println!("{USAGE}");
            return Some(0);
        }
    };
    let attempt: Attempt = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("读不了 {}：{e}", path.display());
            return Some(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return Some(1);
        }
    };
    let mut outcome = Outcome { started_at: now_ms(), ..Default::default() };
    let result = runtime.block_on(run(&attempt, &mut outcome));
    // 模型起的后台命令（连留着的 dev server）都停掉：没人回来点。
    crate::harness::background::stop_all();
    outcome.ended_at = now_ms();
    if let Err(e) = result {
        if outcome.reason.is_empty() {
            outcome.reason = "setup".into();
        }
        outcome.error = Some(e);
    }
    let _ = std::fs::create_dir_all(&attempt.out);
    let written = serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string()).and_then(|t| std::fs::write(attempt.out.join("outcome.json"), t).map_err(|e| e.to_string()));
    Some(match written {
        Ok(()) if outcome.reason != "setup" => 0,
        Ok(()) => 1,
        Err(e) => {
            eprintln!("写不了结果：{e}");
            1
        }
    })
}

async fn run(a: &Attempt, outcome: &mut Outcome) -> Result<(), String> {
    use crate::harness::run as hrun;
    std::fs::create_dir_all(&a.out).map_err(|e| e.to_string())?;
    let mut store = crate::store::Store::open(&crate::mode::app_data_dir().join("peck.db")).map_err(|e| e.to_string())?;
    store.recover_interrupted_runs().map_err(|e| e.to_string())?;
    crate::gateway::prices::load(&store);
    store.set_setting("ui", &a.ui.to_string()).map_err(|e| e.to_string())?;
    let state = crate::state::AppState::new(store).map_err(|e| e.to_string())?;
    crate::harness::mcp_client::set_keys(state.api.clone());
    crate::plugins::seed_library();

    let who = a.ui.pointer("/ploop/main").and_then(Value::as_str).filter(|w| w.starts_with("api:")).ok_or("臂的设置里没有 ploop.main（api:<供应商>/<模型>）。")?.to_string();
    let model_id = hrun::model_id_of(&who)?;
    outcome.model = model_id.clone();
    outcome.effort = effort_for(&model_id, a.ui.get("efforts"));
    let level = hrun::permission_of(Some(&a.permission))?;

    // 打开项目、点开（认结构、装默认插件），再把这些记成一笔：判分从这一笔比。
    let root = a.workspace.canonicalize().map_err(|e| format!("工作区：{e}"))?;
    let ws = crate::workspace::open_workspace(&root.to_string_lossy())?;
    state.store.lock().map_err(|e| e.to_string())?.save_workspace(&ws).map_err(|e| e.to_string())?;
    let prepared = crate::project_plugins::prepare(&state.store, &crate::plugins::library_dir(), &ws.id)?;
    outcome.prepared = prepared.created.into_iter().chain(prepared.installed).collect();
    outcome.base_commit = commit_structure(&root)?;
    let session = state.store.lock().map_err(|e| e.to_string())?.create_session(&ws.id, Some(a.prompt.chars().take(60).collect())).map_err(|e| e.to_string())?;

    let (provider, team, team_notes): (Arc<dyn ModelProvider>, _, _) = match &a.script {
        Some(path) => (Arc::new(ScriptedModel::new(model_id.clone(), script(path)?, Duration::ZERO)), None, vec![]),
        None => {
            real_keys(&state, a, &model_id)?;
            let provider = crate::gateway::providers::for_model(&state, &model_id).await.map_err(|e| e.message)?;
            let (team, notes) = crate::harness::agents::prepare(&state, &model_id).await;
            (provider, Some(Arc::new(team)), notes)
        }
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
    let run_id = hrun::start_peck_run(
        hrun::PeckRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id: session.id.clone(),
            prompt: a.prompt.clone(),
            who,
            permission: Some(a.permission.clone()),
            level,
            model_id,
            effort: outcome.effort.clone(),
            cost_cap: a.cost_cap,
            env: crate::harness::tools::bash::shell_env(),
            worktree: false,
            team,
            team_notes,
            images: vec![],
            search: crate::gateway::search::Keys::from_state(&state),
            compact: None,
        },
        provider,
        move |e: &AgentEvent| {
            let _ = tx.send(e.clone());
        },
    )?;
    outcome.run_id = Some(run_id.clone());

    // 等它做完：卡片自动答；到时间先打断，再等一会儿还不完就不等了（调用方会杀掉、记成 infra）。
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(a.timeout_secs);
    let mut interrupted = false;
    let mut ended = false;
    let mut wrap_up = false;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            e = rx.recv() => match e {
                None => break,
                Some(e) => match e.body {
                    EventBody::ApprovalRequested { request_id, high_risk, .. } => {
                        outcome.approvals_denied += 1;
                        outcome.high_risk_denied += u32::from(high_risk);
                        let _ = crate::gateway::runner::gateway_control(run_id.clone(), Control::Approve { request_id, decision: ApprovalDecision::DenyOnce }).await;
                    }
                    EventBody::UserInputRequested { request_id, .. } => {
                        outcome.asks_answered += 1;
                        let _ = crate::gateway::runner::gateway_control(run_id.clone(), Control::Answer { request_id, answer: a.ask_reply.clone(), answers: vec![] }).await;
                    }
                    EventBody::TurnCompleted { reason, error, .. } if e.subagent_id.is_none() => {
                        ended = true;
                        if !interrupted {
                            outcome.reason = serde_json::to_value(reason).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                        }
                        outcome.error = error;
                    }
                    _ => {}
                },
            },
            _ = tick.tick() => {
                let idle = state.active_runs.lock().map(|r| r.is_empty()).unwrap_or(true);
                if ended && idle {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    // 已经做完、在收尾（快照、Git Plan 提交）：再等一会儿，不算超时。
                    if ended && !wrap_up {
                        wrap_up = true;
                        deadline = tokio::time::Instant::now() + Duration::from_secs(60);
                        continue;
                    }
                    if interrupted || wrap_up {
                        outcome.reason = "timeout".into();
                        return Err("到时间了，又等了 60 秒还没停。".into());
                    }
                    interrupted = true;
                    outcome.reason = "timeout".into();
                    let _ = crate::gateway::runner::gateway_control(run_id.clone(), Control::Interrupt).await;
                    deadline = tokio::time::Instant::now() + Duration::from_secs(60);
                }
            }
        }
    }
    if outcome.reason.is_empty() {
        outcome.reason = "error".into();
        outcome.error = Some("事件流断了，没等到 turn.completed。".into());
    }
    let db = state.store.lock().map_err(|e| e.to_string())?;
    let events = db.agent_events(&run_id).map_err(|e| e.to_string())?;
    let mut lines = String::new();
    for e in &events {
        lines.push_str(&serde_json::to_string(e).map_err(|e| e.to_string())?);
        lines.push('\n');
    }
    std::fs::write(a.out.join("events.jsonl"), lines).map_err(|e| e.to_string())?;
    outcome.cost_usd = db.harness_spend_since(0).unwrap_or(0.0);
    Ok(())
}

/// 发任务时带的思考强度：照前端 `effortFor`（存着的，没存用默认：有 medium 用 medium、不然取中间那一档；OFF 和不支持的不带）。
fn effort_for(model_id: &str, saved: Option<&Value>) -> Option<String> {
    let spec = crate::gateway::catalog::catalog().model(model_id)?;
    if spec.efforts.is_empty() || spec.thinking == crate::gateway::catalog::Thinking::Never {
        return None;
    }
    let default = || if spec.efforts.iter().any(|e| e == "medium") { "medium".to_string() } else { spec.efforts[(spec.efforts.len() - 1) / 2].clone() };
    let v = saved.and_then(|s| s.get(format!("api:{model_id}"))).and_then(Value::as_str).map(str::to_string).unwrap_or_else(default);
    (v != "off" && spec.efforts.contains(&v)).then_some(v)
}

/// 点开项目时建的、装的文件记成一笔（不是 git 仓库、没东西可记就是现在的 HEAD）。不动 git 配置：身份只给这一次。
fn commit_structure(root: &Path) -> Result<Option<String>, String> {
    if !root.join(".git").exists() {
        return Ok(None);
    }
    let git = |args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("git {}：{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    git(&["add", "-A"])?;
    if !git(&["status", "--porcelain"])?.is_empty() {
        git(&["-c", "user.name=peck-eval", "-c", "user.email=eval@peck.invalid", "commit", "-q", "--no-verify", "-m", "peck: project structure"])?;
    }
    git(&["rev-parse", "HEAD"]).map(Some)
}

/// 剧本：`{"turns": [{"text", "reasoning", "tools": [["read_file", {…}]], "usage": {"input", "output", "cacheRead", "cacheWrite"}}]}`。
fn script(path: &Path) -> Result<Vec<ScriptTurn>, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        turns: Vec<Turn>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Turn {
        #[serde(default)]
        reasoning: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        tools: Vec<(String, Value)>,
        #[serde(default)]
        usage: crate::gateway::event::TokenUsage,
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("剧本 {}：{e}", path.display()))?;
    let file: File = serde_json::from_str(&text).map_err(|e| format!("剧本 {}：{e}", path.display()))?;
    Ok(file.turns.into_iter().map(|t| ScriptTurn { reasoning: t.reasoning, text: t.text, tool_calls: t.tools, usage: t.usage }).collect())
}

/// 真跑：只用内存里的 Key（不读写钥匙串、不弹框），从 Key 目录按文件名读主角色、子代理要的那几家，Tavily 有就带上。
fn real_keys(state: &crate::state::AppState, a: &Attempt, main: &str) -> Result<(), String> {
    let dir = a.keys.as_ref().ok_or("真跑要给 Key 目录（keys）。")?;
    *state.api.mode.lock().map_err(|e| e.to_string())? = crate::mode::ApiMode::Real;
    let mut keys = crate::api::keys::Keys::new(None);
    let mut models = vec![main.to_string()];
    if let Some(kinds) = a.ui.get("subagents").and_then(Value::as_object) {
        models.extend(kinds.values().filter_map(|k| k.get("model").and_then(Value::as_str)).map(|m| m.trim_start_matches("api:").to_string()));
    }
    let cat = crate::gateway::catalog::catalog();
    for m in &models {
        let spec = cat.model(m).ok_or_else(|| format!("模型目录里没有 {m}。"))?;
        let file = if spec.provider == "google" { "gemini" } else { spec.provider.as_str() };
        if keys.cached(&spec.provider).is_some() {
            continue;
        }
        let key = std::fs::read_to_string(dir.join(file)).map_err(|_| format!("Key 目录里没有 {file}。"))?;
        keys.set(&spec.provider, Some(key.trim().to_string()))?;
    }
    if let Ok(key) = std::fs::read_to_string(dir.join("tavily")) {
        keys.set("tavily", Some(key.trim().to_string()))?;
    }
    *state.api.keys.lock().map_err(|e| e.to_string())? = keys;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn effort_follows_the_frontend() {
        let spec = crate::gateway::catalog::catalog().model("deepseek/deepseek-flash").unwrap();
        let fallback = if spec.efforts.iter().any(|e| e == "medium") { "medium".to_string() } else { spec.efforts[(spec.efforts.len() - 1) / 2].clone() };
        assert_eq!(effort_for("deepseek/deepseek-flash", None), Some(fallback), "没存用默认");
        let first = spec.efforts[0].clone();
        assert_eq!(effort_for("deepseek/deepseek-flash", Some(&json!({ "api:deepseek/deepseek-flash": first.clone() }))), Some(first));
        assert_eq!(effort_for("deepseek/deepseek-flash", Some(&json!({ "api:deepseek/deepseek-flash": "off" }))), None, "OFF 不带");
        assert_eq!(effort_for("deepseek/deepseek-flash", Some(&json!({ "api:deepseek/deepseek-flash": "nope" }))), None, "不支持的不带");
    }

    #[test]
    fn scripts_read_turns_and_reject_typos() {
        let dir = std::env::temp_dir().join(format!("peck-eval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.json");
        std::fs::write(&ok, r#"{"turns":[{"text":"看一下","tools":[["read_file",{"path":"a.rs"}]],"usage":{"input":10,"output":2,"cacheRead":5}}]}"#).unwrap();
        let turns = script(&ok).unwrap();
        assert_eq!(turns[0].tool_calls[0].0, "read_file");
        assert_eq!(turns[0].usage.cache_read, Some(5));
        let typo = dir.join("typo.json");
        std::fs::write(&typo, r#"{"turns":[{"txt":"拼错了"}]}"#).unwrap();
        assert!(script(&typo).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
