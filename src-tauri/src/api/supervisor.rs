use super::{
    profiles::Protocol,
    tools,
    worker::{self, ApiTask, RunUsage},
    ApiState,
};
use crate::{
    events::{EventSink, RunEvent, StepDescriptor, StepStatus},
    snapshot::ShadowRepo,
    store::{now_ms, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub const INSTRUCTIONS:&str="Plan briefly, then delegate bounded implementation to the configured Worker using delegate_task. Supply only its goal, necessary context, write paths and acceptance criteria, never your private reasoning or full conversation. Paths include any needed generated/dependency files; '.' grants the whole workspace. Workers run serially inside this run and cannot delegate. Inspect the returned files, diff references, command results and usage; use your own read/search/command tools as needed. Explicitly call review_task for each latest child: accept only a completed verified task; rework with concrete feedback before delegate_task(retryTaskId); or takeover and implement/validate with your own tools. Rework/delegation limits and the run-wide request/tool/token budget are hard limits. A failed Worker is evidence, not project success. After takeover run a successful validation command. Do not finish while a latest child is unreviewed or still marked rework. Your final answer reports actual outcomes and remaining uncertainty. Never execute external model CLIs or delegate outside Peck.";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub decision: String,
    pub feedback: String,
    pub reviewed_at: i64,
}

pub fn definitions(protocol: &Protocol) -> Vec<Value> {
    [
        ("delegate_task","Run the configured Worker synchronously inside the SAME root run. Provide title, goal, necessary context, allowed workspace-relative write paths and explicit acceptance criteria. No parent reasoning or whole transcript is forwarded. Returns child task ID, final status/summary, real changed files, diff reference, command validation and usage, not private reasoning. Worker cannot delegate. To rework, first review_task(decision='rework'), then pass retryTaskId; attempts and total delegations are bounded. Inspect and review the returned result before finishing.",json!({"type":"object","properties":{"title":{"type":"string"},"goal":{"type":"string"},"context":{"type":"string"},"paths":{"type":"array","items":{"type":"string"},"minItems":1},"acceptance":{"type":"string"},"retryTaskId":{"type":"string"}},"required":["title","goal","context","paths","acceptance"]})),
        ("review_task","Record an explicit review of a direct child task. decision is accept, rework or takeover. Give concrete evidence/feedback. accept requires completed child; rework requires remaining attempts and a later delegate_task(retryTaskId); takeover requires Supervisor implementation plus a successful subsequent command validation. Review each latest child before your final response. This tool never executes another model.",json!({"type":"object","properties":{"taskId":{"type":"string"},"decision":{"type":"string","enum":["accept","rework","takeover"]},"feedback":{"type":"string"}},"required":["taskId","decision","feedback"]})),
    ].into_iter().map(|(name,description,schema)|match protocol{
        Protocol::Anthropic=>json!({"name":name,"description":description,"input_schema":schema}),
        Protocol::Responses=>json!({"type":"function","name":name,"description":description,"parameters":schema,"strict":false}),
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen=>json!({"type":"function","function":{"name":name,"description":description,"parameters":schema}}),
    }).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Delegation {
    title: String,
    goal: String,
    context: String,
    paths: Vec<String>,
    acceptance: String,
    retry_task_id: Option<String>,
}

pub async fn delegate(
    parent: &ApiTask,
    args: &Value,
    sink: &EventSink,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    cancel: &mut watch::Receiver<bool>,
    repo: &ShadowRepo,
    usage: &mut RunUsage,
) -> Result<String, String> {
    if parent.settings.role != "supervisor" || parent.parent_id.is_some() {
        return Err("只有根 Supervisor 可委派，Worker 不支持递归委派。".into());
    }
    if *cancel.borrow() {
        return Err("运行已取消，未创建子任务。".into());
    }
    if usage.delegations >= parent.limits.max_delegations {
        return Err("已达到本次运行委派上限；Supervisor 可检查并接管，或暂停。".into());
    }
    let d: Delegation =
        serde_json::from_value(args.clone()).map_err(|e| format!("委派参数无效：{e}"))?;
    if d.title.trim().is_empty()
        || d.title.len() > 200
        || d.goal.trim().is_empty()
        || d.goal.len() > 20_000
        || d.context.len() > 24_000
        || d.acceptance.trim().is_empty()
        || d.acceptance.len() > 8_000
        || d.paths.is_empty()
        || d.paths.len() > 20
    {
        return Err("委派需要明确目标、上下文、1～20 个写入路径及验收条件，内容不能超长。".into());
    }
    let tasks = store
        .lock()
        .map_err(|e| e.to_string())?
        .api_tasks(&parent.run_id)
        .map_err(|e| e.to_string())?;
    let previous = if let Some(id) = &d.retry_task_id {
        let old = tasks
            .iter()
            .find(|t| t.id == *id && t.parent_id.as_deref() == Some(&parent.id))
            .ok_or("返工只能引用本 Supervisor 的直接子任务。")?;
        if tasks
            .iter()
            .any(|t| t.previous_task_id.as_deref() == Some(id))
        {
            return Err("该子任务已有后续尝试，请审查最新结果。".into());
        }
        if old.attempt > parent.limits.max_reworks {
            return Err("返工次数已用完，请接管或暂停。".into());
        }
        if old.review.as_ref().map(|r| r.decision.as_str()) != Some("rework") {
            return Err("请先 review_task 标记返工并给出反馈。".into());
        }
        Some(old)
    } else {
        None
    };
    let mut paths = vec![];
    for p in &d.paths {
        let resolved = tools::workspace_path(&parent.workspace_path, p)?;
        let relative = resolved
            .strip_prefix(&parent.workspace_path)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        paths.push(if relative.is_empty() {
            ".".into()
        } else {
            relative
        });
    }
    let settings = parent
        .worker_settings
        .clone()
        .ok_or("缺少 Worker 配置快照。")?;
    let model = parent
        .worker_model
        .clone()
        .ok_or("缺少 Worker 模型快照。")?;
    let provider = match &parent.worker_provider {
        Some(provider) => provider.clone(),
        None => store.lock().map_err(|e| e.to_string())?
            .api_config().map_err(|e| e.to_string())?.providers()
            .into_iter().find(|p| p.id == model.provider_id)
            .ok_or("Worker 供应商不存在。")?,
    };
    let attempt = previous.map(|t| t.attempt + 1).unwrap_or(1);
    let retry = previous
        .map(|t| {
            format!(
                "\nPrevious result (not replayable): {}\nReview feedback: {}",
                tools::clipped(t.summary.clone().unwrap_or_default(), 6000),
                t.review.as_ref().map(|r| r.feedback.as_str()).unwrap_or("")
            )
        })
        .unwrap_or_default();
    let prompt=format!("Goal: {}\nNecessary context: {}\nAllowed write paths: {}\nAcceptance: {}{}\nInspect current files before editing; previous actions are not replayed.",d.goal,d.context,paths.join(", "),d.acceptance,retry);
    let id = uuid::Uuid::new_v4().to_string();
    let child = ApiTask {
        id: id.clone(),
        parent_id: Some(parent.id.clone()),
        previous_task_id: d.retry_task_id,
        step_id: format!("task-{id}"),
        title: d.title,
        paths,
        acceptance: d.acceptance,
        attempt,
        review: None,
        changeset: Default::default(),
        worker_model: None,
        worker_settings: None,
        worker_provider: None,
        run_id: parent.run_id.clone(),
        workspace_path: parent.workspace_path.clone(),
        mode: parent.mode,
        model,
        settings,
        limits: parent.limits.clone(),
        scenario: match parent.scenario.as_str() {
            "takeover" => "workerFailure",
            "childSlow" => "slow",
            "rework" if attempt > 1 => "revision",
            _ => "success",
        }
        .into(),
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
    usage.delegations += 1;
    let index = {
        let db = store.lock().map_err(|e| e.to_string())?;
        db.insert_worker_task(&child).map_err(|e| e.to_string())?;
        db.run(&parent.run_id)
            .map_err(|e| e.to_string())?
            .steps
            .len()
    };
    sink.emit(RunEvent::StepAdded {
        run_id: parent.run_id.clone(),
        step: StepDescriptor {
            id: child.step_id.clone(),
            step_index: index,
            agent_id: "api-worker".into(),
            agent_name: child.model.name.clone(),
            title: child.title.clone(),
            status: StepStatus::Pending,
            expects: vec![],
            exit_code: None,
            duration_ms: None,
            started_at: None,
            finished_at: None,
            execution_mode: None,
            notice: None,
        },
    });
    let child = Box::pin(worker::execute_task(
        child, provider, sink, api, store, cancel, repo, usage,
    ))
    .await;
    Ok(result_summary(&child).to_string())
}

pub fn review(
    parent: &ApiTask,
    args: &Value,
    store: &Arc<Mutex<Store>>,
    sink: &EventSink,
) -> Result<String, String> {
    if parent.settings.role != "supervisor" || parent.parent_id.is_some() {
        return Err("只有根 Supervisor 可审查子任务。".into());
    }
    let id = args["taskId"].as_str().ok_or("缺少 taskId。")?;
    let decision = args["decision"].as_str().ok_or("缺少 decision。")?;
    let feedback = args["feedback"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 8000)
        .ok_or("审查需给出具体反馈，最多 8000 字节。")?;
    let db = store.lock().map_err(|e| e.to_string())?;
    let tasks = db.api_tasks(&parent.run_id).map_err(|e| e.to_string())?;
    let mut child = tasks
        .iter()
        .find(|t| t.id == id && t.parent_id.as_deref() == Some(&parent.id))
        .cloned()
        .ok_or("未找到该 Supervisor 的子任务。")?;
    if tasks
        .iter()
        .any(|t| t.previous_task_id.as_deref() == Some(id))
    {
        return Err("请审查该任务最新的返工结果。".into());
    }
    match decision {
        "accept" if child.status != "completed" => {
            return Err("未完成或失败的子任务不能验收通过，请返工或接管。".into())
        }
        "rework" if child.attempt > parent.limits.max_reworks => {
            return Err("返工次数已用完，请接管或暂停。".into())
        }
        "accept" | "rework" | "takeover" => {}
        _ => return Err("审查决定无效。".into()),
    }
    child.review = Some(Review {
        decision: decision.into(),
        feedback: feedback.into(),
        reviewed_at: now_ms(),
    });
    db.save_worker_task(&child).map_err(|e| e.to_string())?;
    drop(db);
    sink.emit(RunEvent::StepExecution {
        run_id: parent.run_id.clone(),
        step_id: child.step_id.clone(),
        mode: if parent.mode == crate::mode::ApiMode::Real {
            "api-real"
        } else {
            "api-mock"
        }
        .into(),
        notice: format!(
            "Worker · {} · 审查 {}：{}",
            child.model.api_model,
            decision,
            tools::clipped(feedback.into(), 800)
        ),
    });
    Ok(json!({"taskId":id,"decision":decision,"feedback":feedback}).to_string())
}

pub fn verify_completion(parent: &ApiTask, store: &Arc<Mutex<Store>>) -> Result<(), String> {
    let tasks = store
        .lock()
        .map_err(|e| e.to_string())?
        .api_tasks(&parent.run_id)
        .map_err(|e| e.to_string())?;
    for child in tasks.iter().filter(|t| {
        t.parent_id.as_deref() == Some(&parent.id)
            && !tasks
                .iter()
                .any(|n| n.previous_task_id.as_deref() == Some(&t.id))
    }) {
        match child.review.as_ref() {
            Some(r) if r.decision == "accept" && child.status == "completed" => {}
            Some(r)
                if r.decision == "takeover"
                    && parent.tools.iter().any(|t| {
                        t.tool.name == "run_command"
                            && t.status == "completed"
                            && t.finished_at.is_some_and(|at| at >= r.reviewed_at)
                    }) => {}
            _ => {
                return Err(format!(
                    "子任务「{}」尚未验收或接管验证未完成，整次任务不能标成功。",
                    child.title
                ))
            }
        }
    }
    Ok(())
}

pub fn result_summary(task: &ApiTask) -> Value {
    let validation=task.tools.iter().filter(|t|t.tool.name=="run_command").rev().take(4).map(|t|json!({"command":t.tool.arguments["command"],"status":t.status,"result":tools::clipped(t.output.clone(),2000)})).collect::<Vec<_>>();
    json!({"taskId":task.id,"status":task.status,"summary":tools::clipped(task.summary.clone().unwrap_or_default(),8000),"attempt":task.attempt,"paths":task.paths,"acceptance":task.acceptance,"changeset":task.changeset,"diffRef":{"runId":task.run_id,"stepId":task.step_id},"validation":validation,"usage":{"inputTokens":task.input_tokens,"outputTokens":task.output_tokens,"calls":task.call_ids.len()},"canRetry":task.attempt<=task.limits.max_reworks,"next":"Inspect evidence, then review_task: accept / rework / takeover. No private reasoning is included."})
}

pub fn mock_calls(round: u32, scenario: &str, messages: &[Value]) -> Vec<(&'static str, Value)> {
    let results = messages
        .iter()
        .flat_map(|m| {
            if m["type"] == "function_call_output" {
                vec![m["output"].clone()]
            } else if m["role"] == "tool" {
                vec![m["content"].clone()]
            } else {
                m["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "tool_result")
                    .map(|b| b["content"].clone())
                    .collect()
            }
        })
        .filter_map(|v| serde_json::from_str::<Value>(v.as_str().unwrap_or("")).ok())
        .collect::<Vec<_>>();
    let latest = results.iter().rev().find(|v| v["attempt"].is_number());
    let id = latest.and_then(|v| v["taskId"].as_str()).unwrap_or("");
    let delegation = json!({"title":"Worker 文件实现","goal":"创建并验证 peck-worker-demo.txt","context":"只做模拟工具验证，不是台账项目；使用真实文件工具。","paths":["."],"acceptance":"文件存在、内容正确，验证命令退出0。"});
    match (scenario, round) {
        (_, 1) => vec![("delegate_task", delegation)],
        ("rework", 2) => vec![(
            "review_task",
            json!({"taskId":id,"decision":"rework","feedback":"请将示例内容更新为返工完成，再重新验证。"}),
        )],
        ("rework", 3) => {
            let mut d = delegation;
            d["retryTaskId"] = json!(id);
            vec![("delegate_task", d)]
        }
        ("rework", 4) => vec![(
            "review_task",
            json!({"taskId":id,"decision":"accept","feedback":"已核对第二次文件快照和命令结果，通过。"}),
        )],
        ("takeover", 2) => vec![
            (
                "review_task",
                json!({"taskId":id,"decision":"takeover","feedback":"Worker 验证失败，由 Supervisor 接管并执行有限验证。"}),
            ),
            (
                "apply_patch",
                json!({"edits":[{"path":"peck-supervisor-demo.txt","oldText":"","newText":"Supervisor 接管完成\n"}]}),
            ),
            (
                "run_command",
                json!({"command":"test -f peck-supervisor-demo.txt && cat peck-supervisor-demo.txt"}),
            ),
        ],
        (_, 2) => vec![
            (
                "review_task",
                json!({"taskId":id,"decision":"accept","feedback":"已核对 Worker 的文件改动和成功验证命令。"}),
            ),
            ("read_file", json!({"path":"peck-worker-demo.txt"})),
        ],
        _ => vec![],
    }
}
