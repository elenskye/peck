//! Muse 的会话协议（MSP，`muse serve`，stdio JSON-RPC）→ 统一事件。协议按本机版本：
//! `muse schema generate-json-schema --out DIR`。
//!
//! - 过程是 `item/started` / `item/delta` / `item/completed`（`kind`：`agentMessage`、`reasoning`、
//!   `toolCall`……），回合是 `turn/started` / `turn/completed`。
//! - 审批：Muse 自己的 LLM 审查员先看，拿不准的才发 `approval/request` 给 Peck（同一件事可能还有
//!   `approval/requested` 通知，按 `approvalId` 只出一次）；回答走 `approval/decide`。审查员自己定了的，
//!   这里出一条提示。
//! - 提问：`userInput/request`，回答走 `userInput/answer`。
//! - 用量：`session/tokenUsage` 的累计；上下文占用跟 `session/contextUsage`；额度：`usage/changed`。

use super::{raw_kind, relative, short, Emitted, LineConverter};
use crate::gateway::caps::AgentCaps;
use crate::gateway::event::{
    ApprovalDecision, Cost, EventBody, FileChange, FileDiff, NoticeLevel, PlanStatus, PlanStep,
    QuotaWindow, TokenUsage, ToolKind, ToolStatus, TurnEndReason, UsageScope,
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub struct MuseConverter {
    cwd: Option<String>,
    asked: HashSet<String>,
    context: (Option<u64>, Option<u64>),
    last_quota: Option<Value>,
}

impl MuseConverter {
    pub fn new(cwd: Option<String>) -> Self {
        Self {
            cwd,
            asked: HashSet::new(),
            context: (None, None),
            last_quota: None,
        }
    }

    /// 审批在统一事件里的 id：`muse-<approvalId>`。
    pub fn approval_id(approval: &str) -> String {
        format!("muse-{approval}")
    }

    /// 提问在统一事件里的 id：`muse-input-<userInputId>`。
    pub fn input_id(user_input: &str) -> String {
        format!("muse-input-{user_input}")
    }

    /// 统一的审批选择 → Muse 给的选项里对应的那个 `choiceId`。没有完全对上的就退一步：
    /// 「总是允许」退成「这次允许」，「总是拒绝」退成「这次拒绝」（拒绝最后退成 `abort`）。
    pub fn choice_for(decision: ApprovalDecision, choices: &Value) -> Option<String> {
        let list = choices.as_array()?;
        let find = |want: &[(&str, Option<&str>)]| {
            want.iter().find_map(|(d, scope)| {
                list.iter()
                    .find(|c| c["decision"] == *d && scope.is_none_or(|s| c["scope"] == s))
                    .and_then(|c| c["choiceId"].as_str().map(str::to_string))
            })
        };
        match decision {
            ApprovalDecision::AllowOnce => find(&[("approved", Some("once")), ("approved", None)]),
            ApprovalDecision::AllowAlways => find(&[
                ("approvedForSession", None),
                ("approvedPolicyAmendment", None),
                ("approved", None),
            ]),
            ApprovalDecision::DenyOnce => find(&[("denied", Some("once")), ("denied", None), ("abort", None)]),
            ApprovalDecision::DenyAlways => find(&[("deniedPolicyAmendment", None), ("denied", None), ("abort", None)]),
        }
    }

    fn rel(&self, path: &str) -> String {
        relative(path, self.cwd.as_deref())
    }

    fn tool_kind_and_title(&self, tool: &str, args: &Value) -> (ToolKind, String) {
        let path = args["path"]
            .as_str()
            .or(args["file_path"].as_str())
            .map(|p| self.rel(p))
            .unwrap_or_default();
        let t = tool.to_ascii_lowercase();
        if t == "bash" || t.contains("shell") || t.contains("exec") {
            let cmd = args["command"].as_str().unwrap_or("");
            return (ToolKind::Run, format!("跑 {}", short(cmd, 80)));
        }
        if t.starts_with("read") || t == "view" || t == "cat" {
            return (ToolKind::Read, format!("读 {path}"));
        }
        if t.contains("write") || t.contains("edit") || t.contains("patch") || t.contains("replace") {
            return (ToolKind::Edit, if path.is_empty() { "改文件".into() } else { format!("改 {path}") });
        }
        if t.contains("grep") || t.contains("glob") || t.contains("search") || t.contains("find") || t == "ls" || t.contains("list") {
            let what = args["pattern"].as_str().or(args["query"].as_str()).unwrap_or(&path);
            return (ToolKind::Search, format!("搜 {}", short(what, 60)));
        }
        if t.contains("fetch") || t.contains("web") {
            let url = args["url"].as_str().unwrap_or("");
            return (ToolKind::Fetch, format!("取 {}", short(url, 60)));
        }
        if t.contains("todo") || t.contains("plan") {
            return (ToolKind::Other, "更新任务清单".into());
        }
        (ToolKind::Other, tool.to_string())
    }

    fn item(&mut self, item: &Value, completed: bool) -> Vec<EventBody> {
        let id = item["itemId"].as_str().unwrap_or("").to_string();
        match item["kind"].as_str().unwrap_or("") {
            "agentMessage" if completed => vec![EventBody::MessageCompleted {
                item_id: id,
                text: item["text"].as_str().unwrap_or("").into(),
            }],
            "reasoning" if completed => {
                let summary = item["summary"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n"))
                    .unwrap_or_default();
                let text = if summary.is_empty() { item["text"].as_str().unwrap_or("").to_string() } else { summary };
                vec![EventBody::ReasoningCompleted {
                    item_id: id,
                    text,
                    opaque: None,
                }]
            }
            "toolCall" => {
                let tool = item["tool"].as_str().unwrap_or("");
                let args: Value = item["args"]
                    .as_str()
                    .and_then(|a| serde_json::from_str(a).ok())
                    .unwrap_or_else(|| json!({}));
                if !completed {
                    let (kind, title) = self.tool_kind_and_title(tool, &args);
                    return vec![EventBody::ToolCallStarted {
                        call_id: id,
                        name: tool.into(),
                        kind,
                        title,
                        input: args,
                    }];
                }
                let output = item["visibleOutput"].as_str().map(str::to_string);
                // bash 的输出是一段 JSON（带 exit_code）。
                let exit_code = output
                    .as_deref()
                    .and_then(|o| serde_json::from_str::<Value>(o).ok())
                    .and_then(|v| v["exit_code"].as_i64())
                    .map(|c| c as i32);
                let status = match item["status"].as_str() {
                    Some("completed") if exit_code.is_some_and(|c| c != 0) => ToolStatus::Failed,
                    Some("completed") => ToolStatus::Ok,
                    Some("rejected") => ToolStatus::Denied,
                    _ => ToolStatus::Failed,
                };
                let diff = match (&item["patchSummary"], args["path"].as_str().or(args["file_path"].as_str())) {
                    (s, Some(path)) if s.is_object() => vec![FileDiff {
                        path: self.rel(path),
                        change: FileChange::Modified,
                        old_path: None,
                        additions: s["added"].as_u64().map(|n| n as u32),
                        deletions: s["removed"].as_u64().map(|n| n as u32),
                        patch: None,
                    }],
                    _ => vec![],
                };
                vec![EventBody::ToolCallCompleted {
                    call_id: id,
                    status,
                    output: output.or_else(|| item["failureReason"].as_str().map(str::to_string)),
                    exit_code,
                    diff,
                    duration_ms: item["durationMs"].as_u64(),
                }]
            }
            // 站主自己的那一句、Muse 内部的提醒子任务、压缩记录：不出事件。
            "userMessage" | "reminderChild" | "compaction" | "agentMessage" | "reasoning" => vec![],
            other => {
                if completed {
                    vec![EventBody::Raw {
                        source: "muse".into(),
                        kind: Some(format!("item/{other}")),
                    }]
                } else {
                    vec![]
                }
            }
        }
    }

    fn approval(&mut self, p: &Value) -> Vec<EventBody> {
        let Some(approval) = p["approvalId"].as_str() else {
            return vec![];
        };
        if !self.asked.insert(approval.to_string()) {
            return vec![];
        }
        let subject = &p["subject"];
        let (kind, title) = match subject["kind"].as_str() {
            Some("shell") => (ToolKind::Run, format!("跑 {}", short(subject["command"].as_str().unwrap_or(""), 80))),
            Some("network") | Some("web") => (
                ToolKind::Fetch,
                format!("连 {}", subject["host"].as_str().or(subject["target"].as_str()).unwrap_or("网络")),
            ),
            Some("write") | Some("file") | Some("filesystem") => (
                ToolKind::Edit,
                format!("改 {}", self.rel(subject["path"].as_str().unwrap_or(""))),
            ),
            _ => {
                let args: Value = p["rawArgs"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or_default();
                self.tool_kind_and_title(p["toolName"].as_str().unwrap_or(""), &args)
            }
        };
        let mut options = Vec::new();
        for d in [
            ApprovalDecision::AllowOnce,
            ApprovalDecision::AllowAlways,
            ApprovalDecision::DenyOnce,
            ApprovalDecision::DenyAlways,
        ] {
            let choice = Self::choice_for(d, &p["availableChoices"]);
            // 「总是」退回成「这次」的不重复列。
            let dup = choice.is_some()
                && options.iter().any(|o: &ApprovalDecision| Self::choice_for(*o, &p["availableChoices"]) == choice);
            if choice.is_some() && !dup {
                options.push(d);
            }
        }
        vec![EventBody::ApprovalRequested {
            request_id: Self::approval_id(approval),
            call_id: p["toolCallId"].as_str().or(p["itemId"].as_str()).map(str::to_string),
            kind,
            title,
            detail: subject["command"]
                .as_str()
                .map(str::to_string)
                .or_else(|| p["reviewerReason"].as_str().map(str::to_string)),
            high_risk: false,
            options,
        }]
    }

    fn quota(&mut self, p: &Value) -> Vec<EventBody> {
        let mut windows = Vec::new();
        let w = &p["window"];
        if w.is_object() {
            windows.push(QuotaWindow {
                label: match w["windowDurationMins"].as_u64() {
                    Some(300) => "5 小时".into(),
                    Some(m) if m % 60 == 0 => format!("{} 小时", m / 60),
                    Some(m) => format!("{m} 分钟"),
                    None => "额度".into(),
                },
                used_percent: w["usedPercent"].as_f64(),
                resets_at: w["resetsAtMs"].as_i64(),
            });
        }
        let wk = &p["weekly"];
        if wk.is_object() {
            windows.push(QuotaWindow {
                label: "每周".into(),
                used_percent: wk["usedPercent"].as_f64(),
                resets_at: wk["resetsAtMs"].as_i64(),
            });
        }
        let key = serde_json::to_value(&windows).unwrap_or_default();
        if windows.is_empty() || self.last_quota.as_ref() == Some(&key) {
            return vec![];
        }
        self.last_quota = Some(key);
        vec![EventBody::QuotaUpdated {
            provider: "muse".into(),
            windows,
        }]
    }

    pub(crate) fn convert(&mut self, raw: &Value) -> Vec<EventBody> {
        let Some(method) = raw["method"].as_str() else {
            // 对 Peck 请求的回应：出错的才算事。
            if let Some(message) = raw["error"]["message"].as_str() {
                return vec![EventBody::Error {
                    message: format!("Muse：{}", short(message, 200)),
                    fatal: false,
                }];
            }
            return vec![];
        };
        let p = &raw["params"];
        match method {
            "session/started" => {
                let s = &p["session"];
                vec![EventBody::SessionStarted {
                    engine: "muse".into(),
                    model: s["modelId"].as_str().map(str::to_string),
                    caps: AgentCaps::muse(),
                    tools: vec![],
                    mcp: vec![],
                    session_ref: s["sessionId"].as_str().map(str::to_string),
                    version: None,
                }]
            }
            "turn/started" => vec![EventBody::TurnStarted {
                turn_id: p["turnId"].as_str().unwrap_or("").into(),
            }],
            "turn/completed" => {
                let reason = match p["terminal"].as_str() {
                    Some("completed") => TurnEndReason::Done,
                    Some("cancelled") => TurnEndReason::Interrupted,
                    _ => TurnEndReason::Error,
                };
                vec![EventBody::TurnCompleted {
                    turn_id: p["turnId"].as_str().unwrap_or("").into(),
                    reason,
                    error: p["error"]["message"].as_str().map(str::to_string),
                }]
            }
            "item/started" => self.item(&p["item"], false),
            "item/completed" => self.item(&p["item"], true),
            "item/delta" => {
                let item_id = p["itemId"].as_str().unwrap_or("").to_string();
                let text = p["delta"].as_str().unwrap_or("").to_string();
                if text.is_empty() {
                    return vec![];
                }
                match p["field"].as_str().unwrap_or("text") {
                    "text" => vec![EventBody::MessageDelta { item_id, text }],
                    "output" => vec![EventBody::ToolCallOutputDelta { call_id: item_id, text }],
                    f if f.starts_with("summary") => vec![EventBody::ReasoningDelta { item_id, text }],
                    _ => vec![],
                }
            }
            "approval/request" | "approval/requested" => self.approval(p),
            "approval/resolved" => match p["resolvedBy"].as_str() {
                // Peck 自己回答的由引擎那一层发 approval.resolved；这里只说审查员和规则定了的。
                Some("llmJudge") | Some("policy") => {
                    let allowed = p["policyResult"] == "allow";
                    let who = if p["resolvedBy"] == "llmJudge" { "审查员" } else { "规则" };
                    vec![EventBody::Notice {
                        level: if allowed { NoticeLevel::Info } else { NoticeLevel::Warn },
                        text: format!("Muse 的{who}{}了一次操作", if allowed { "放行" } else { "拦下" }),
                    }]
                }
                _ => vec![],
            },
            "userInput/request" => {
                let Some(id) = p["userInputId"].as_str() else {
                    return vec![];
                };
                let questions = p["questions"].as_array().cloned().unwrap_or_default();
                let first = questions.first().cloned().unwrap_or(Value::Null);
                vec![EventBody::UserInputRequested {
                    request_id: Self::input_id(id),
                    question: questions
                        .iter()
                        .filter_map(|q| q["question"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    options: first["options"]
                        .as_array()
                        .map(|o| o.iter().filter_map(|x| x["label"].as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                    free_text: true,
                }]
            }
            "session/contextUsage" => {
                self.context = (p["usedTokens"].as_u64(), p["windowTokens"].as_u64());
                vec![]
            }
            "session/tokenUsage" => {
                let c = &p["cumulative"];
                let read = c["cacheReadTokens"].as_u64();
                let write = c["cacheWriteTokens"].as_u64();
                vec![EventBody::UsageUpdated {
                    usage: TokenUsage {
                        input: c["promptTokens"]
                            .as_u64()
                            .map(|t| t.saturating_sub(read.unwrap_or(0) + write.unwrap_or(0))),
                        output: c["outputTokens"].as_u64(),
                        cache_read: read,
                        cache_write: write,
                        reasoning: None,
                        context_used: self.context.0,
                        context_window: self.context.1,
                    },
                    cost: Cost::unknown(),
                    scope: UsageScope::Cumulative,
                }]
            }
            "usage/changed" => self.quota(p),
            "session/todoListChanged" => {
                let steps = p["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|i| i["status"] != "cancelled")
                    .map(|i| PlanStep {
                        text: i["text"].as_str().unwrap_or("").into(),
                        status: match i["status"].as_str() {
                            Some("completed") => PlanStatus::Completed,
                            Some("inProgress") => PlanStatus::InProgress,
                            _ => PlanStatus::Pending,
                        },
                    })
                    .collect();
                vec![EventBody::PlanUpdated {
                    steps,
                    explanation: None,
                }]
            }
            "turn/retryScheduled" | "turn/modelRetryScheduled" => vec![EventBody::Notice {
                level: NoticeLevel::Warn,
                text: "Muse 在重试这一步".into(),
            }],
            // 状态类：不出事件。
            "session/approvalModeChanged" | "session/nameChanged" | "session/statusChanged"
            | "turn/modelAttemptStarted" | "turn/foregroundCompleted" | "approval/reviewing"
            | "approval/updated" | "item/updated" | "session/modelChanged" | "session/reasoningEffortChanged"
            | "session/viewHealthChanged" | "session/listChanged" | "skill/changed" | "plugin/changed"
            | "model/completed" | "recap/completed" | "session/goalChanged" | "session/branchChanged" => vec![],
            _ => vec![EventBody::Raw {
                source: "muse".into(),
                kind: raw_kind(raw, &["method"]),
            }],
        }
    }
}

impl LineConverter for MuseConverter {
    fn feed(&mut self, raw: &Value) -> Vec<Emitted> {
        self.convert(raw).into_iter().map(Into::into).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::{convert_jsonl, fixtures};

    #[test]
    fn muse_small_task_fixture() {
        let mut c = MuseConverter::new(Some("{{WORKSPACE}}".into()));
        let events = convert_jsonl(&mut c, "muse", &fixtures::read("muse-small-task", "raw.jsonl"));
        fixtures::check("muse-small-task", &events);
        let bodies: Vec<&EventBody> = events.iter().map(|e| &e.body).collect();
        assert!(matches!(bodies.first(), Some(EventBody::SessionStarted { .. })));
        assert!(matches!(bodies.last(), Some(EventBody::TurnCompleted { reason: TurnEndReason::Done, .. })));
        let titles: Vec<&str> = bodies
            .iter()
            .filter_map(|b| match b {
                EventBody::ToolCallStarted { title, .. } => Some(title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, vec!["读 notes.txt", "跑 echo muse > stamp.txt"]);
        assert!(bodies.iter().any(|b| matches!(b, EventBody::QuotaUpdated { windows, .. } if windows.len() == 2)));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::Notice { .. })), "审查员放行要有一条提示");
        assert!(!bodies.iter().any(|b| matches!(b, EventBody::Raw { .. })), "录音里不该有读不懂的");
    }

    #[test]
    fn escalated_approvals_ask_once_and_map_choices() {
        let mut c = MuseConverter::new(None);
        let choices = json!([
            {"choiceId": "allow_once", "decision": "approved", "scope": "once", "label": "Allow once"},
            {"choiceId": "allow_session", "decision": "approvedForSession", "scope": "session", "label": "Allow for session"},
            {"choiceId": "abort", "decision": "abort", "scope": "once", "label": "Reject"}
        ]);
        let req = json!({"method": "approval/request", "id": 7, "params": {"approvalId": "a1", "toolCallId": "t1", "toolName": "bash",
            "subject": {"kind": "shell", "command": "rm -rf build"}, "availableChoices": choices, "judgeEscalated": true}});
        let out = c.convert(&req);
        let EventBody::ApprovalRequested { request_id, options, title, .. } = &out[0] else { panic!("要弹审批") };
        assert_eq!(request_id, "muse-a1");
        assert_eq!(title, "跑 rm -rf build");
        assert_eq!(options, &vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce]);
        let again = json!({"method": "approval/requested", "params": {"approvalId": "a1"}});
        assert!(c.convert(&again).is_empty(), "同一件事只问一次");
        assert_eq!(MuseConverter::choice_for(ApprovalDecision::DenyAlways, &choices).as_deref(), Some("abort"));
        assert_eq!(MuseConverter::choice_for(ApprovalDecision::AllowAlways, &choices).as_deref(), Some("allow_session"));
    }
}
