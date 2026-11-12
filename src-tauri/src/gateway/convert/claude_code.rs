//! Claude Code（`claude -p --input-format stream-json --output-format stream-json --verbose`）→ 统一事件。
//!
//! - 审批不用 MCP：加 `--permission-prompt-tool stdio`，Claude Code 把每次审批发成
//!   `control_request`（`can_use_tool`），Peck 用 [`ClaudeCodeConverter::approval_reply`] 回答。
//! - 子任务（Task 工具）里的消息带 `parent_tool_use_id`，转出来的事件带上子任务 id。
//! - `result` 里的花费是 Claude Code 自己估的、续接以后是累计值：`estimated` + `cumulative`。

use super::{relative, short, Emitted, LineConverter};
use crate::gateway::caps::AgentCaps;
use crate::gateway::event::{
    ApprovalDecision, Cost, CostSource, EventBody, FileChange, FileDiff, McpServer, NoticeLevel,
    QuotaWindow, TokenUsage, ToolKind, ToolStatus, TurnEndReason, UsageScope,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct ClaudeCodeConverter {
    cwd: Option<String>,
    turn: u32,
    in_turn: bool,
    /// 工具调用 id → (工具名, 参数)，结果回来时要知道是哪种工具。
    calls: HashMap<String, (String, Value)>,
    /// 流式（`--include-partial-messages`）时当前消息的 id。
    streaming_message: Option<String>,
    last_quota: Option<Value>,
}

impl ClaudeCodeConverter {
    pub fn new(cwd: Option<String>) -> Self {
        Self {
            cwd,
            turn: 0,
            in_turn: false,
            calls: HashMap::new(),
            streaming_message: None,
            last_quota: None,
        }
    }

    /// 发一句话（stream-json 输入的一行）。
    pub fn user_message(text: &str) -> Value {
        json!({"type": "user", "message": {"role": "user", "content": text}, "parent_tool_use_id": null, "session_id": ""})
    }

    /// 回答一次 `can_use_tool`。允许时原样放回它要用的参数。
    pub fn approval_reply(request_id: &str, decision: ApprovalDecision, input: &Value) -> Value {
        let response = if decision.allows() {
            json!({"behavior": "allow", "updatedInput": input})
        } else {
            json!({"behavior": "deny", "message": "站主拒绝了。"})
        };
        json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})
    }

    fn rel(&self, path: &str) -> String {
        relative(path, self.cwd.as_deref())
    }

    fn turn_start(&mut self, out: &mut Vec<Emitted>) {
        if !self.in_turn {
            self.in_turn = true;
            self.turn += 1;
            out.push(
                EventBody::TurnStarted {
                    turn_id: format!("turn-{}", self.turn),
                }
                .into(),
            );
        }
    }

    fn tool_kind_and_title(&self, name: &str, input: &Value) -> (ToolKind, String) {
        let path = |k: &str| self.rel(input[k].as_str().unwrap_or(""));
        match name {
            "Read" | "NotebookRead" => (ToolKind::Read, format!("读 {}", path("file_path"))),
            "Edit" | "MultiEdit" => (ToolKind::Edit, format!("改 {}", path("file_path"))),
            "Write" => (ToolKind::Edit, format!("写 {}", path("file_path"))),
            "NotebookEdit" => (ToolKind::Edit, format!("改 {}", path("notebook_path"))),
            "Bash" | "BashOutput" => (
                ToolKind::Run,
                format!("跑 {}", short(input["command"].as_str().unwrap_or(""), 80)),
            ),
            "Grep" => (
                ToolKind::Search,
                format!("搜 {}", short(input["pattern"].as_str().unwrap_or(""), 60)),
            ),
            "Glob" => (
                ToolKind::Search,
                format!("找文件 {}", short(input["pattern"].as_str().unwrap_or(""), 60)),
            ),
            "WebFetch" => (
                ToolKind::Fetch,
                format!("取网页 {}", short(input["url"].as_str().unwrap_or(""), 60)),
            ),
            "WebSearch" => (
                ToolKind::Fetch,
                format!("搜网页 {}", short(input["query"].as_str().unwrap_or(""), 60)),
            ),
            "Task" | "Agent" => (
                ToolKind::Other,
                format!("子任务 {}", short(input["description"].as_str().unwrap_or(""), 60)),
            ),
            n if n.starts_with("mcp__") => (ToolKind::Mcp, format!("MCP {}", n.trim_start_matches("mcp__"))),
            n => (ToolKind::Other, n.to_string()),
        }
    }

    fn assistant(&mut self, raw: &Value, sub: &Option<String>, out: &mut Vec<Emitted>) {
        let line_id = raw["uuid"].as_str().unwrap_or("").to_string();
        let blocks = raw["message"]["content"].as_array().cloned().unwrap_or_default();
        for (i, block) in blocks.iter().enumerate() {
            let item_id = if blocks.len() > 1 { format!("{line_id}:{i}") } else { line_id.clone() };
            let body = match block["type"].as_str().unwrap_or("") {
                "text" => EventBody::MessageCompleted {
                    item_id,
                    text: block["text"].as_str().unwrap_or("").into(),
                },
                // Claude Code 自己负责回放签名，Peck 不拿它的私有数据。
                "thinking" | "redacted_thinking" => EventBody::ReasoningCompleted {
                    item_id,
                    text: block["thinking"].as_str().unwrap_or("").into(),
                    opaque: None,
                },
                "tool_use" => {
                    let id = block["id"].as_str().unwrap_or("").to_string();
                    let name = block["name"].as_str().unwrap_or("").to_string();
                    let input = block["input"].clone();
                    let (kind, title) = self.tool_kind_and_title(&name, &input);
                    self.calls.insert(id.clone(), (name.clone(), input.clone()));
                    if name == "Task" || name == "Agent" {
                        out.push(Emitted {
                            subagent_id: sub.clone(),
                            body: EventBody::ToolCallStarted {
                                call_id: id.clone(),
                                name,
                                kind,
                                title: title.clone(),
                                input,
                            },
                        });
                        EventBody::SubagentStarted {
                            subagent_id: id.clone(),
                            parent_call_id: Some(id),
                            title,
                        }
                    } else {
                        EventBody::ToolCallStarted {
                            call_id: id,
                            name,
                            kind,
                            title,
                            input,
                        }
                    }
                }
                other => EventBody::Raw {
                    source: "claude-code".into(),
                    kind: Some(format!("assistant/{other}")),
                },
            };
            out.push(Emitted {
                subagent_id: sub.clone(),
                body,
            });
        }
    }

    fn tool_results(&mut self, raw: &Value, sub: &Option<String>, out: &mut Vec<Emitted>) {
        let Some(blocks) = raw["message"]["content"].as_array() else {
            return; // 站主自己发的话（字符串），不出事件。
        };
        let denied: Vec<&str> = raw["tool_result_meta"]
            .as_array()
            .map(|m| {
                m.iter()
                    .filter(|x| matches!(x["permission_decision"]["decision"].as_str(), Some("deny" | "reject")))
                    .filter_map(|x| x["id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        for block in blocks.iter().filter(|b| b["type"] == "tool_result") {
            let call_id = block["tool_use_id"].as_str().unwrap_or("").to_string();
            let output = match &block["content"] {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            let status = if denied.contains(&call_id.as_str()) {
                ToolStatus::Denied
            } else if block["is_error"].as_bool().unwrap_or(false) {
                ToolStatus::Failed
            } else {
                ToolStatus::Ok
            };
            let (name, _) = self.calls.remove(&call_id).unwrap_or_default();
            let diff = self.file_diff(&name, &raw["tool_use_result"]);
            out.push(Emitted {
                subagent_id: sub.clone(),
                body: EventBody::ToolCallCompleted {
                    call_id: call_id.clone(),
                    status,
                    output: Some(output),
                    exit_code: None,
                    diff,
                    duration_ms: None,
                },
            });
            if name == "Task" || name == "Agent" {
                out.push(Emitted {
                    subagent_id: sub.clone(),
                    body: EventBody::SubagentCompleted {
                        subagent_id: call_id,
                        status,
                    },
                });
            }
        }
    }

    /// Edit / Write 的结果里有 `structuredPatch`，拼成 unified diff。
    fn file_diff(&self, tool: &str, result: &Value) -> Vec<FileDiff> {
        if !matches!(tool, "Edit" | "MultiEdit" | "Write") {
            return vec![];
        }
        let Some(file) = result["filePath"].as_str() else {
            return vec![];
        };
        let path = self.rel(file);
        let mut patch = format!("--- a/{path}\n+++ b/{path}\n");
        let (mut add, mut del) = (0u32, 0u32);
        for hunk in result["structuredPatch"].as_array().into_iter().flatten() {
            patch.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk["oldStart"], hunk["oldLines"], hunk["newStart"], hunk["newLines"]
            ));
            for line in hunk["lines"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if line.starts_with('+') {
                    add += 1;
                } else if line.starts_with('-') {
                    del += 1;
                }
                patch.push_str(line);
                patch.push('\n');
            }
        }
        let created = result["type"] == "create";
        if created && add == 0 {
            add = result["content"].as_str().map(|c| c.lines().count() as u32).unwrap_or(0);
        }
        vec![FileDiff {
            path,
            change: if created { FileChange::Added } else { FileChange::Modified },
            old_path: None,
            additions: Some(add),
            deletions: Some(del),
            patch: (add + del > 0).then_some(patch),
        }]
    }

    fn quota(&mut self, info: &Value) -> Vec<Emitted> {
        let labels = [("five_hour", "5 小时"), ("seven_day", "每周"), ("seven_day_opus", "每周 · Opus")];
        let windows: Vec<QuotaWindow> = labels
            .iter()
            .filter_map(|(k, label)| {
                let w = &info["unifiedWindows"][*k];
                w.is_object().then(|| QuotaWindow {
                    label: (*label).into(),
                    used_percent: w["utilization"].as_f64().map(|u| (u * 1000.0).round() / 10.0),
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
            provider: "claude-code".into(),
            windows,
        }
        .into()]
    }

    fn result(&mut self, raw: &Value, out: &mut Vec<Emitted>) {
        let u = &raw["usage"];
        let n = |k: &str| u[k].as_u64();
        let context_window = raw["modelUsage"]
            .as_object()
            .and_then(|m| m.values().find_map(|v| v["contextWindow"].as_u64()));
        out.push(
            EventBody::UsageUpdated {
                usage: TokenUsage {
                    input: n("input_tokens"),
                    output: n("output_tokens"),
                    cache_read: n("cache_read_input_tokens"),
                    cache_write: n("cache_creation_input_tokens"),
                    reasoning: u["output_tokens_details"]["thinking_tokens"].as_u64(),
                    context_used: None,
                    context_window,
                },
                cost: match raw["total_cost_usd"].as_f64() {
                    Some(usd) => Cost {
                        usd: Some(usd),
                        source: CostSource::Estimated,
                    },
                    None => Cost::unknown(),
                },
                scope: UsageScope::Cumulative,
            }
            .into(),
        );
        if let Some(denials) = raw["permission_denials"].as_array().filter(|d| !d.is_empty()) {
            out.push(
                EventBody::Notice {
                    level: NoticeLevel::Warn,
                    text: format!("有 {} 次工具调用没批下来", denials.len()),
                }
                .into(),
            );
        }
        let reason = match raw["subtype"].as_str() {
            Some("success") => TurnEndReason::Done,
            Some("error_max_turns") | Some("error_max_budget_usd") => TurnEndReason::Limit,
            _ if raw["stop_reason"] == "refusal" => TurnEndReason::Refused,
            _ => TurnEndReason::Error,
        };
        let error = (reason == TurnEndReason::Error)
            .then(|| raw["result"].as_str().or(raw["subtype"].as_str()).unwrap_or("出错了").to_string());
        out.push(
            EventBody::TurnCompleted {
                turn_id: format!("turn-{}", self.turn.max(1)),
                reason,
                error,
            }
            .into(),
        );
        self.in_turn = false;
    }

    /// `--include-partial-messages` 的增量。item id 和整条消息不一样（那边用行的 uuid），界面按「同一回合最后一条完成的为准」。
    fn stream_event(&mut self, raw: &Value, sub: &Option<String>) -> Vec<Emitted> {
        let e = &raw["event"];
        match e["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.streaming_message = e["message"]["id"].as_str().map(str::to_string);
                vec![]
            }
            "content_block_delta" => {
                let item_id = format!(
                    "{}:{}",
                    self.streaming_message.as_deref().unwrap_or("stream"),
                    e["index"].as_u64().unwrap_or(0)
                );
                let d = &e["delta"];
                let body = match d["type"].as_str() {
                    Some("text_delta") => EventBody::MessageDelta {
                        item_id,
                        text: d["text"].as_str().unwrap_or("").into(),
                    },
                    Some("thinking_delta") => EventBody::ReasoningDelta {
                        item_id,
                        text: d["thinking"].as_str().unwrap_or("").into(),
                    },
                    _ => return vec![],
                };
                vec![Emitted {
                    subagent_id: sub.clone(),
                    body,
                }]
            }
            _ => vec![],
        }
    }
}

impl LineConverter for ClaudeCodeConverter {
    fn feed(&mut self, raw: &Value) -> Vec<Emitted> {
        let mut out = Vec::new();
        let sub = raw["parent_tool_use_id"].as_str().map(str::to_string);
        match raw["type"].as_str().unwrap_or("") {
            "system" => match raw["subtype"].as_str().unwrap_or("") {
                "init" => {
                    if self.cwd.is_none() {
                        self.cwd = raw["cwd"].as_str().map(str::to_string);
                    }
                    out.push(
                        EventBody::SessionStarted {
                            engine: "claude-code".into(),
                            model: raw["model"].as_str().map(str::to_string),
                            caps: AgentCaps::claude_code(),
                            tools: raw["tools"]
                                .as_array()
                                .map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                                .unwrap_or_default(),
                            mcp: raw["mcp_servers"]
                                .as_array()
                                .map(|m| {
                                    m.iter()
                                        .map(|s| McpServer {
                                            name: s["name"].as_str().unwrap_or("").into(),
                                            status: s["status"].as_str().unwrap_or("").into(),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            session_ref: raw["session_id"].as_str().map(str::to_string),
                            version: raw["claude_code_version"].as_str().map(str::to_string),
                        }
                        .into(),
                    );
                    self.turn_start(&mut out);
                }
                "compact_boundary" => out.push(
                    EventBody::Notice {
                        level: NoticeLevel::Info,
                        text: "Claude Code 压缩了上下文".into(),
                    }
                    .into(),
                ),
                // 状态类：不出事件。
                "commands_changed" | "task_summary" | "thinking_tokens" | "post_turn_summary" | "status"
                | "hook_started" | "hook_response" | "hook_progress" => {}
                other => out.push(
                    EventBody::Raw {
                        source: "claude-code".into(),
                        kind: Some(format!("system/{other}")),
                    }
                    .into(),
                ),
            },
            "assistant" => {
                self.turn_start(&mut out);
                self.assistant(raw, &sub, &mut out);
            }
            "user" => {
                self.turn_start(&mut out);
                self.tool_results(raw, &sub, &mut out);
            }
            "stream_event" => out.extend(self.stream_event(raw, &sub)),
            "result" => self.result(raw, &mut out),
            "rate_limit_event" => out.extend(self.quota(&raw["rate_limit_info"])),
            "control_request" => {
                let request = &raw["request"];
                match request["subtype"].as_str() {
                    Some("can_use_tool") => {
                        let tool = request["tool_name"].as_str().unwrap_or("");
                        let input = &request["input"];
                        let (kind, title) = self.tool_kind_and_title(tool, input);
                        let mut options = vec![ApprovalDecision::AllowOnce];
                        if request["permission_suggestions"].as_array().is_some_and(|s| !s.is_empty()) {
                            options.push(ApprovalDecision::AllowAlways);
                        }
                        options.push(ApprovalDecision::DenyOnce);
                        out.push(
                            EventBody::ApprovalRequested {
                                request_id: raw["request_id"].as_str().unwrap_or("").into(),
                                call_id: request["tool_use_id"].as_str().map(str::to_string),
                                kind,
                                title,
                                detail: input["command"]
                                    .as_str()
                                    .or(input["file_path"].as_str())
                                    .map(str::to_string),
                                high_risk: false,
                                options,
                            }
                            .into(),
                        );
                    }
                    other => out.push(
                        EventBody::Raw {
                            source: "claude-code".into(),
                            kind: Some(format!("control_request/{}", other.unwrap_or("?"))),
                        }
                        .into(),
                    ),
                }
            }
            "control_response" | "keep_alive" => {}
            other => out.push(
                EventBody::Raw {
                    source: "claude-code".into(),
                    kind: Some(other.to_string()),
                }
                .into(),
            ),
        }
        out
    }

    fn finish(&mut self) -> Vec<Emitted> {
        // 进程没给 result 就退了：补一条出错的回合结束。
        if self.in_turn {
            self.in_turn = false;
            return vec![EventBody::TurnCompleted {
                turn_id: format!("turn-{}", self.turn),
                reason: TurnEndReason::Error,
                error: Some("Claude Code 中途退出了".into()),
            }
            .into()];
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{convert_jsonl, fixtures};
    use super::*;

    #[test]
    fn claude_code_small_task_fixture() {
        let raw = fixtures::read("claude-code-small-task", "raw.jsonl");
        let mut converter = ClaudeCodeConverter::new(None);
        let events = convert_jsonl(&mut converter, "claude-code", &raw);
        fixtures::check("claude-code-small-task", &events);
        let types: Vec<&str> = events.iter().map(|e| e.body.type_name()).collect();
        assert_eq!(types.first(), Some(&"session.started"));
        assert_eq!(types.last(), Some(&"turn.completed"));
        assert!(!types.contains(&"raw"), "夹具里不该有不认识的事件：{types:?}");
        let titles: Vec<String> = events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::ToolCallStarted { title, .. } => Some(title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["读 notes.txt", "改 notes.txt", "跑 echo done > stamp.txt"]);
        let edit = events.iter().find_map(|e| match &e.body {
            EventBody::ToolCallCompleted { diff, .. } if !diff.is_empty() => Some(diff[0].clone()),
            _ => None,
        });
        assert_eq!(edit.map(|d| (d.path, d.additions)), Some(("notes.txt".into(), Some(1))));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::UsageUpdated { cost, .. } if cost.source == CostSource::Estimated)));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::QuotaUpdated { provider, .. } if provider == "claude-code")));
    }

    #[test]
    fn approvals_and_subagents_map_through() {
        let mut c = ClaudeCodeConverter::new(Some("/ws".into()));
        let ask = c.feed(&json!({"type": "control_request", "request_id": "r1", "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "t1",
            "input": {"command": "rm -rf build"}, "permission_suggestions": [{"type": "addRules"}]}}));
        assert!(matches!(&ask[0].body, EventBody::ApprovalRequested { title, options, detail, .. }
            if title == "跑 rm -rf build" && options.len() == 3 && detail.as_deref() == Some("rm -rf build")));
        let reply = ClaudeCodeConverter::approval_reply("r1", ApprovalDecision::DenyOnce, &json!({}));
        assert_eq!(reply["response"]["response"]["behavior"], "deny");

        let task = c.feed(&json!({"type": "assistant", "uuid": "u1", "parent_tool_use_id": null, "message": {"content": [
            {"type": "tool_use", "id": "task1", "name": "Task", "input": {"description": "查一下依赖"}}]}}));
        assert!(task.iter().any(|e| matches!(&e.body, EventBody::SubagentStarted { subagent_id, .. } if subagent_id == "task1")));
        let inner = c.feed(&json!({"type": "assistant", "uuid": "u2", "parent_tool_use_id": "task1", "message": {"content": [
            {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "/ws/Cargo.toml"}}]}}));
        assert_eq!(inner[0].subagent_id.as_deref(), Some("task1"));
        assert!(matches!(&inner[0].body, EventBody::ToolCallStarted { title, .. } if title == "读 Cargo.toml"));
        let done = c.feed(&json!({"type": "user", "uuid": "u3", "parent_tool_use_id": null, "message": {"content": [
            {"type": "tool_result", "tool_use_id": "task1", "content": [{"type": "text", "text": "好了"}]}]}}));
        assert!(done.iter().any(|e| matches!(&e.body, EventBody::SubagentCompleted { status: ToolStatus::Ok, .. })));
        assert_eq!(c.finish().len(), 1, "没有 result 就结束要补一条出错的回合结束");
    }
}
