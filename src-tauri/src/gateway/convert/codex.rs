//! Codex `app-server`（stdio JSON-RPC）→ 统一事件。只看服务端发来的行（通知和服务端请求）；
//! Peck 回的审批由引擎那一层发 `approval.resolved`。协议按本机版本：`codex app-server generate-ts`。

use super::{count_lines, raw_kind, relative, short, split_unified_diff, LineConverter};
use crate::gateway::caps::AgentCaps;
use crate::gateway::event::{
    ApprovalDecision, Cost, EventBody, FileChange, FileDiff, NoticeLevel, PlanStatus, PlanStep,
    QuotaWindow, TokenUsage, ToolKind, ToolStatus, TurnEndReason, UsageScope,
};
use serde_json::{json, Value};

pub struct CodexConverter {
    /// 项目根（绝对路径），用来把路径变成相对的。
    cwd: Option<String>,
    last_quota: Option<Value>,
}

impl CodexConverter {
    pub fn new(cwd: Option<String>) -> Self {
        Self {
            cwd,
            last_quota: None,
        }
    }

    /// 审批请求的 id 在统一事件里写成 `codex-<JSON-RPC id>`，回答时去掉前缀。
    pub fn request_id(rpc_id: &Value) -> String {
        match rpc_id {
            Value::String(s) => format!("codex-{s}"),
            other => format!("codex-{other}"),
        }
    }

    /// 统一的审批选择 → Codex 的回答。
    pub fn decision_reply(decision: ApprovalDecision) -> Value {
        match decision {
            ApprovalDecision::AllowOnce => json!({"decision": "accept"}),
            ApprovalDecision::AllowAlways => json!({"decision": "acceptForSession"}),
            ApprovalDecision::DenyOnce | ApprovalDecision::DenyAlways => json!({"decision": "decline"}),
        }
    }

    fn rel(&self, path: &str) -> String {
        relative(path, self.cwd.as_deref())
    }

    fn item(&self, item: &Value, completed: bool) -> Vec<EventBody> {
        let id = str_of(item, "id");
        match item["type"].as_str().unwrap_or("") {
            "agentMessage" if completed => vec![EventBody::MessageCompleted {
                item_id: id,
                text: str_of(item, "text"),
            }],
            "reasoning" if completed => {
                let parts = |k: &str| {
                    item[k]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n"))
                        .unwrap_or_default()
                };
                let summary = parts("summary");
                vec![EventBody::ReasoningCompleted {
                    item_id: id,
                    text: if summary.is_empty() { parts("content") } else { summary },
                    opaque: None,
                }]
            }
            "commandExecution" => {
                if completed {
                    let exit_code = item["exitCode"].as_i64().map(|c| c as i32);
                    let status = match item["status"].as_str() {
                        Some("declined") => ToolStatus::Denied,
                        Some("failed") => ToolStatus::Failed,
                        _ if exit_code.is_some_and(|c| c != 0) => ToolStatus::Failed,
                        _ => ToolStatus::Ok,
                    };
                    vec![EventBody::ToolCallCompleted {
                        call_id: id,
                        status,
                        output: item["aggregatedOutput"].as_str().map(str::to_string),
                        exit_code,
                        diff: vec![],
                        duration_ms: item["durationMs"].as_u64(),
                    }]
                } else {
                    let (kind, title) = self.command_title(item);
                    vec![EventBody::ToolCallStarted {
                        call_id: id,
                        name: "commandExecution".into(),
                        kind,
                        title,
                        input: json!({"command": item["command"], "cwd": self.rel(item["cwd"].as_str().unwrap_or(""))}),
                    }]
                }
            }
            "fileChange" => {
                let changes = item["changes"].as_array().cloned().unwrap_or_default();
                if completed {
                    let diff = changes.iter().map(|c| self.file_change(c)).collect();
                    let status = match item["status"].as_str() {
                        Some("declined") => ToolStatus::Denied,
                        Some("failed") => ToolStatus::Failed,
                        _ => ToolStatus::Ok,
                    };
                    vec![EventBody::ToolCallCompleted {
                        call_id: id,
                        status,
                        output: None,
                        exit_code: None,
                        diff,
                        duration_ms: None,
                    }]
                } else {
                    let paths: Vec<String> = changes
                        .iter()
                        .map(|c| self.rel(c["path"].as_str().unwrap_or("")))
                        .collect();
                    vec![EventBody::ToolCallStarted {
                        call_id: id,
                        name: "fileChange".into(),
                        kind: ToolKind::Edit,
                        title: format!("改 {}", short(&paths.join("、"), 60)),
                        input: json!({"paths": paths}),
                    }]
                }
            }
            "mcpToolCall" => {
                let name = format!("{}.{}", str_of(item, "server"), str_of(item, "tool"));
                if completed {
                    let failed = item["status"] == "failed" || !item["error"].is_null();
                    let output = if failed {
                        item["error"]["message"].as_str().map(str::to_string)
                    } else {
                        Some(mcp_text(&item["result"]))
                    };
                    vec![EventBody::ToolCallCompleted {
                        call_id: id,
                        status: if failed { ToolStatus::Failed } else { ToolStatus::Ok },
                        output,
                        exit_code: None,
                        diff: vec![],
                        duration_ms: item["durationMs"].as_u64(),
                    }]
                } else {
                    vec![EventBody::ToolCallStarted {
                        call_id: id,
                        title: format!("MCP {name}"),
                        name,
                        kind: ToolKind::Mcp,
                        input: item["arguments"].clone(),
                    }]
                }
            }
            "webSearch" => {
                if completed {
                    vec![EventBody::ToolCallCompleted {
                        call_id: id,
                        status: ToolStatus::Ok,
                        output: None,
                        exit_code: None,
                        diff: vec![],
                        duration_ms: None,
                    }]
                } else {
                    vec![EventBody::ToolCallStarted {
                        call_id: id,
                        name: "webSearch".into(),
                        kind: ToolKind::Fetch,
                        title: format!("搜网页 {}", short(item["query"].as_str().unwrap_or(""), 60)),
                        input: json!({"query": item["query"]}),
                    }]
                }
            }
            // 自己发的话、开头的 agentMessage（等 delta）、开头的 reasoning：不出事件。
            "userMessage" | "agentMessage" | "reasoning" | "hookPrompt" => vec![],
            other => vec![EventBody::Raw {
                source: "codex".into(),
                kind: Some(format!("item/{other}")),
            }],
        }
    }

    fn command_title(&self, item: &Value) -> (ToolKind, String) {
        let actions = item["commandActions"].as_array().cloned().unwrap_or_default();
        let types: Vec<&str> = actions.iter().filter_map(|a| a["type"].as_str()).collect();
        let first = actions.first();
        if !types.is_empty() && types.iter().all(|t| *t == "read") {
            let names: Vec<String> = actions
                .iter()
                .map(|a| a["name"].as_str().unwrap_or("").to_string())
                .collect();
            return (ToolKind::Read, format!("读 {}", short(&names.join("、"), 60)));
        }
        if !types.is_empty() && types.iter().all(|t| *t == "listFiles") {
            return (ToolKind::Read, "列出文件".into());
        }
        if !types.is_empty() && types.iter().all(|t| *t == "search") {
            let query = first.and_then(|a| a["query"].as_str()).unwrap_or("");
            return (ToolKind::Search, format!("搜 {}", short(query, 60)));
        }
        let shown = if actions.len() == 1 {
            first.and_then(|a| a["command"].as_str()).unwrap_or("")
        } else {
            item["command"].as_str().unwrap_or("")
        };
        (ToolKind::Run, format!("跑 {}", short(shown, 80)))
    }

    fn file_change(&self, change: &Value) -> FileDiff {
        let patch = change["diff"].as_str().unwrap_or("").to_string();
        let (additions, deletions) = count_lines(&patch);
        let path = self.rel(change["path"].as_str().unwrap_or(""));
        let (change_kind, old_path, path) = match change["kind"]["type"].as_str() {
            Some("add") => (FileChange::Added, None, path),
            Some("delete") => (FileChange::Deleted, None, path),
            _ => match change["kind"]["move_path"].as_str() {
                Some(to) => (FileChange::Renamed, Some(path), self.rel(to)),
                None => (FileChange::Modified, None, path),
            },
        };
        FileDiff {
            path,
            change: change_kind,
            old_path,
            additions: Some(additions),
            deletions: Some(deletions),
            patch: (!patch.is_empty()).then_some(patch),
        }
    }

    fn quota(&mut self, limits: &Value) -> Vec<EventBody> {
        let windows: Vec<QuotaWindow> = ["primary", "secondary"]
            .iter()
            .filter_map(|k| {
                let w = &limits[*k];
                w.is_object().then(|| QuotaWindow {
                    label: window_label(w["windowDurationMins"].as_u64()),
                    used_percent: w["usedPercent"].as_f64(),
                    resets_at: w["resetsAt"].as_i64().map(|s| s * 1000),
                })
            })
            .collect();
        let key = serde_json::to_value(&windows).unwrap_or_default();
        if windows.is_empty() || self.last_quota.as_ref() == Some(&key) {
            return vec![];
        }
        self.last_quota = Some(key);
        vec![EventBody::QuotaUpdated {
            provider: "codex".into(),
            windows,
        }]
    }
}

fn str_of(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn window_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "5 小时".into(),
        Some(10080) => "每周".into(),
        Some(1440) => "每天".into(),
        Some(m) if m % 60 == 0 => format!("{} 小时", m / 60),
        Some(m) => format!("{m} 分钟"),
        None => "额度".into(),
    }
}

fn mcp_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn usage(tokens: &Value, window: Option<u64>, context_used: Option<u64>) -> TokenUsage {
    let n = |k: &str| tokens[k].as_u64();
    let cached = n("cachedInputTokens").unwrap_or(0);
    let cache_write = n("cacheWriteInputTokens").unwrap_or(0);
    TokenUsage {
        // Codex（OpenAI 口径）的 inputTokens 含缓存，减掉才是没走缓存的。
        input: n("inputTokens").map(|t| t.saturating_sub(cached + cache_write)),
        output: n("outputTokens"),
        cache_read: Some(cached),
        cache_write: Some(cache_write),
        reasoning: n("reasoningOutputTokens"),
        context_used,
        context_window: window,
    }
}

impl LineConverter for CodexConverter {
    fn feed(&mut self, raw: &Value) -> Vec<super::Emitted> {
        // Codex 的子任务是另开的 thread，事件里不混在一起；这里都算主线。
        self.convert(raw).into_iter().map(Into::into).collect()
    }
}

impl CodexConverter {
    fn convert(&mut self, raw: &Value) -> Vec<EventBody> {
        let method = raw["method"].as_str();
        let params = &raw["params"];
        // 服务端请求（带 id 和 method）：审批、问站主。
        if let (Some(method), Some(id)) = (method, raw.get("id")) {
            let request_id = Self::request_id(id);
            return match method {
                "item/commandExecution/requestApproval" | "execCommandApproval" => {
                    let command = params["commandActions"][0]["command"]
                        .as_str()
                        .or(params["command"].as_str())
                        .unwrap_or("");
                    vec![EventBody::ApprovalRequested {
                        request_id,
                        call_id: params["itemId"].as_str().map(str::to_string),
                        kind: ToolKind::Run,
                        title: format!("跑 {}", short(command, 80)),
                        detail: params["reason"]
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| params["command"].as_str().map(str::to_string)),
                        high_risk: false,
                        options: approval_options(&params["availableDecisions"]),
                    }]
                }
                "item/fileChange/requestApproval" | "applyPatchApproval" => {
                    vec![EventBody::ApprovalRequested {
                        request_id,
                        call_id: params["itemId"].as_str().map(str::to_string),
                        kind: ToolKind::Edit,
                        title: "改文件".into(),
                        detail: params["reason"].as_str().map(str::to_string),
                        high_risk: params["grantRoot"].is_string(),
                        options: vec![
                            ApprovalDecision::AllowOnce,
                            ApprovalDecision::AllowAlways,
                            ApprovalDecision::DenyOnce,
                        ],
                    }]
                }
                "item/tool/requestUserInput" => {
                    let questions = params["questions"].as_array().cloned().unwrap_or_default();
                    let first = questions.first().cloned().unwrap_or(Value::Null);
                    vec![EventBody::UserInputRequested {
                        request_id,
                        question: questions
                            .iter()
                            .filter_map(|q| q["question"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        options: first["options"]
                            .as_array()
                            .map(|o| o.iter().filter_map(|x| x["label"].as_str().map(str::to_string)).collect())
                            .unwrap_or_default(),
                        free_text: first["isOther"].as_bool().unwrap_or(true) || first["options"].is_null(),
                    }]
                }
                _ => vec![EventBody::Raw {
                    source: "codex".into(),
                    kind: Some(method.into()),
                }],
            };
        }
        let Some(method) = method else {
            // 对 Peck 请求的回应：出错的才算事。
            if let Some(message) = raw["error"]["message"].as_str() {
                return vec![EventBody::Error {
                    message: message.into(),
                    fatal: false,
                }];
            }
            return vec![];
        };
        match method {
            "thread/started" => {
                let t = &params["thread"];
                vec![EventBody::SessionStarted {
                    engine: "codex".into(),
                    model: t["model"].as_str().map(str::to_string),
                    caps: AgentCaps::codex(),
                    tools: vec![],
                    mcp: vec![],
                    session_ref: t["id"].as_str().map(str::to_string),
                    version: t["cliVersion"].as_str().map(str::to_string),
                }]
            }
            "turn/started" => vec![EventBody::TurnStarted {
                turn_id: str_of(&params["turn"], "id"),
            }],
            "turn/completed" => {
                let turn = &params["turn"];
                let reason = match turn["status"].as_str() {
                    Some("completed") => TurnEndReason::Done,
                    Some("interrupted") => TurnEndReason::Interrupted,
                    _ => TurnEndReason::Error,
                };
                vec![EventBody::TurnCompleted {
                    turn_id: str_of(turn, "id"),
                    reason,
                    error: turn["error"]["message"].as_str().map(str::to_string),
                }]
            }
            "item/started" => self.item(&params["item"], false),
            "item/completed" => self.item(&params["item"], true),
            "item/agentMessage/delta" => vec![EventBody::MessageDelta {
                item_id: str_of(params, "itemId"),
                text: str_of(params, "delta"),
            }],
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                vec![EventBody::ReasoningDelta {
                    item_id: str_of(params, "itemId"),
                    text: str_of(params, "delta"),
                }]
            }
            "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" => {
                vec![EventBody::ToolCallOutputDelta {
                    call_id: str_of(params, "itemId"),
                    text: str_of(params, "delta"),
                }]
            }
            "turn/diff/updated" => vec![EventBody::DiffUpdated {
                files: split_unified_diff(params["diff"].as_str().unwrap_or(""), self.cwd.as_deref()),
            }],
            "turn/plan/updated" => vec![EventBody::PlanUpdated {
                steps: params["plan"]
                    .as_array()
                    .map(|steps| {
                        steps
                            .iter()
                            .map(|s| PlanStep {
                                text: str_of(s, "step"),
                                status: match s["status"].as_str() {
                                    Some("completed") => PlanStatus::Completed,
                                    Some("inProgress") => PlanStatus::InProgress,
                                    _ => PlanStatus::Pending,
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                explanation: params["explanation"].as_str().map(str::to_string),
            }],
            "thread/tokenUsage/updated" => {
                let t = &params["tokenUsage"];
                let last = &t["last"];
                let context_used = match (last["inputTokens"].as_u64(), last["outputTokens"].as_u64()) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                };
                vec![EventBody::UsageUpdated {
                    usage: usage(&t["total"], t["modelContextWindow"].as_u64(), context_used),
                    // 用的是站主的订阅，没有按次的美元。
                    cost: Cost::unknown(),
                    scope: UsageScope::Cumulative,
                }]
            }
            "account/rateLimits/updated" => self.quota(&params["rateLimits"]),
            "error" => vec![EventBody::Error {
                message: params["error"]["message"]
                    .as_str()
                    .or(params["message"].as_str())
                    .unwrap_or("Codex 出错了")
                    .to_string(),
                fatal: !params["willRetry"].as_bool().unwrap_or(false),
            }],
            "warning" | "configWarning" | "deprecationNotice" | "guardianWarning" => {
                vec![EventBody::Notice {
                    level: NoticeLevel::Warn,
                    text: params["message"]
                        .as_str()
                        .or(params["summary"].as_str())
                        .unwrap_or(method)
                        .to_string(),
                }]
            }
            "mcpServer/startupStatus/updated" if params["status"] == "failed" => {
                vec![EventBody::Notice {
                    level: NoticeLevel::Warn,
                    text: format!("Codex 的 MCP {} 没起来", str_of(params, "name")),
                }]
            }
            // 状态类、账号类、自己发的：不出事件。
            "thread/status/changed"
            | "thread/settings/updated"
            | "thread/name/updated"
            | "mcpServer/startupStatus/updated"
            | "serverRequest/resolved"
            | "remoteControl/status/changed"
            | "account/updated"
            | "item/fileChange/patchUpdated"
            | "hook/started"
            | "hook/completed"
            | "skills/changed"
            | "thread/readState/changed"
            | "rawResponseItem/completed"
            | "rawResponse/completed" => vec![],
            _ => vec![EventBody::Raw {
                source: "codex".into(),
                kind: raw_kind(raw, &["method"]),
            }],
        }
    }
}

fn approval_options(available: &Value) -> Vec<ApprovalDecision> {
    let Some(list) = available.as_array() else {
        return vec![
            ApprovalDecision::AllowOnce,
            ApprovalDecision::AllowAlways,
            ApprovalDecision::DenyOnce,
        ];
    };
    let mut out = Vec::new();
    for d in list {
        let mapped = match d.as_str() {
            Some("accept") => Some(ApprovalDecision::AllowOnce),
            Some("acceptForSession") => Some(ApprovalDecision::AllowAlways),
            Some("decline") | Some("cancel") => Some(ApprovalDecision::DenyOnce),
            None if d.get("acceptWithExecpolicyAmendment").is_some() => Some(ApprovalDecision::AllowAlways),
            _ => None,
        };
        if let Some(m) = mapped {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{convert_jsonl, fixtures};
    use super::*;

    #[test]
    fn codex_small_task_fixture() {
        let raw = fixtures::read("codex-small-task", "raw.jsonl");
        let mut converter = CodexConverter::new(Some("{{WORKSPACE}}".into()));
        let events = convert_jsonl(&mut converter, "codex", &raw);
        fixtures::check("codex-small-task", &events);
        let types: Vec<&str> = events.iter().map(|e| e.body.type_name()).collect();
        assert_eq!(types.first(), Some(&"session.started"));
        assert_eq!(types.last(), Some(&"turn.completed"));
        assert_eq!(types.iter().filter(|t| **t == "approval.requested").count(), 3);
        assert!(!types.contains(&"raw"), "夹具里不该有不认识的事件：{types:?}");
    }

    #[test]
    fn unknown_methods_become_raw_and_decisions_map_back() {
        let mut c = CodexConverter::new(None);
        assert_eq!(
            c.feed(&json!({"method": "thread/realtime/started", "params": {}})),
            vec![EventBody::Raw { source: "codex".into(), kind: Some("thread/realtime/started".into()) }.into()]
        );
        assert_eq!(CodexConverter::decision_reply(ApprovalDecision::AllowAlways), json!({"decision": "acceptForSession"}));
        assert_eq!(CodexConverter::request_id(&json!(7)), "codex-7");
    }
}
