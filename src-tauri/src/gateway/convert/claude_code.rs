//! Claude Code（`claude -p --input-format stream-json --output-format stream-json --verbose`）→ 统一事件。
//!
//! - 审批不用 MCP：加 `--permission-prompt-tool stdio`，Claude Code 把每次审批发成
//!   `control_request`（`can_use_tool`），Peck 用 [`ClaudeCodeConverter::approval_reply`] 回答。
//! - 子任务（Task 工具）里的消息带 `parent_tool_use_id`，转出来的事件带上子任务 id。
//! - `result` 里的花费是 Claude Code 自己估的、续接以后是累计值：`estimated` + `cumulative`。
//! - 任务清单（v1.1.3）：旧的 `TodoWrite` 每次给整份；2.1 起默认的 `TaskCreate` / `TaskUpdate` 一次一项（编号在
//!   `TaskCreate` 的结果里），Peck 记着拼成整份，都转成 `plan.updated`。子任务里的清单不转（不盖掉主线的）。

use super::{relative, short, Emitted, LineConverter};
use crate::gateway::caps::AgentCaps;
use crate::gateway::event::{
    ApprovalDecision, Cost, CostSource, EventBody, FileChange, FileDiff, McpServer, NoticeLevel,
    PlanStatus, PlanStep, QuotaWindow, TokenUsage, ToolKind, ToolStatus, TurnEndReason, UsageScope,
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
    /// 流式出过的消息 id → 整条消息里已经来过几块。Claude Code 一块内容出一行 `assistant`（同一个消息 id），
    /// 增量那边的 item id 是「消息 id:第几块」，完成的那条照这个编号，界面才认得是同一段，不会显示两遍。
    streamed_blocks: HashMap<String, usize>,
    last_quota: Option<Value>,
    /// `TaskCreate` / `TaskUpdate` 拼出来的清单：（编号, 一项）。
    tasks: Vec<(String, PlanStep)>,
    /// 上下文用量（v1.1.7）：主线最后一条回答的提示有多大（同一个消息 id 只算一次）、窗口多大（回合末的 `result` 才报）。
    context_used: Option<u64>,
    counted_message: Option<String>,
    context_window: Option<u64>,
}

impl ClaudeCodeConverter {
    pub fn new(cwd: Option<String>) -> Self {
        Self {
            cwd,
            turn: 0,
            in_turn: false,
            calls: HashMap::new(),
            streaming_message: None,
            streamed_blocks: HashMap::new(),
            last_quota: None,
            tasks: Vec::new(),
            context_used: None,
            counted_message: None,
            context_window: None,
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

    /// 回答 `AskUserQuestion`：允许这次调用，答案按问题原文放进 `answers`。
    pub fn answer_reply(request_id: &str, input: &Value, answer: &str, each: &[Vec<String>]) -> Value {
        let mut updated = input.clone();
        // 每个问题自己的答案（v1.1.9；多选的几项用「, 」连起来，Claude Code 的答案是一句话）。
        let answers: serde_json::Map<String, Value> = input["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(i, q)| Some((q["question"].as_str()?.to_string(), json!(crate::gateway::Control::picks(answer, each, i).join(", ")))))
            .collect();
        updated["answers"] = Value::Object(answers);
        Self::approval_reply(request_id, ApprovalDecision::AllowOnce, &updated)
    }

    /// 打断这一回合（stream-json 的控制请求）。
    pub fn interrupt_request(request_id: &str) -> Value {
        json!({"type": "control_request", "request_id": request_id, "request": {"subtype": "interrupt"}})
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
            // Claude Code 的 Bash 带一句 description（v1.1.11）：和 PLOOP 一样拿它当标题，命令点开看。
            "Bash" | "BashOutput" => (
                ToolKind::Run,
                match input["description"].as_str().map(str::trim).filter(|d| !d.is_empty()) {
                    Some(d) => format!("跑：{}", short(d, 80)),
                    None => format!("跑 {}", short(input["command"].as_str().unwrap_or(""), 80)),
                },
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
            "TodoWrite" | "TaskCreate" | "TaskUpdate" => (ToolKind::Other, "更新任务清单".into()),
            // Peck 自己的本机 MCP（v0.9）：和 Peck Loop 一样写。
            "mcp__peck__ask_user" => (ToolKind::Mcp, "问站主".into()),
            "mcp__peck__create_skill" => (ToolKind::Other, format!("想存成 Skill {}", short(input["name"].as_str().unwrap_or(""), 60))),
            n if n.starts_with("mcp__") => (ToolKind::Mcp, format!("MCP {}", n.trim_start_matches("mcp__"))),
            n => (ToolKind::Other, n.to_string()),
        }
    }

    fn assistant(&mut self, raw: &Value, sub: &Option<String>, out: &mut Vec<Emitted>) {
        let line_id = raw["uuid"].as_str().unwrap_or("").to_string();
        let blocks = raw["message"]["content"].as_array().cloned().unwrap_or_default();
        let message_id = raw["message"]["id"].as_str().unwrap_or("").to_string();
        // 上下文用量（v1.1.7）：主线（不是子代理里的）这一次请求的提示 = 输入 + 写缓存 + 读缓存；并行的几块是同一个消息 id，只算一次。
        let u = &raw["message"]["usage"];
        if sub.is_none() && u.is_object() && self.counted_message.as_deref() != Some(message_id.as_str()) {
            let n = |k: &str| u[k].as_u64().unwrap_or(0);
            let used = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
            if used > 0 {
                self.counted_message = Some(message_id.clone());
                self.context_used = Some(used);
                out.push(super::cli_context("Claude Code", used, self.context_window, false).into());
            }
        }
        let streamed_from = self.streamed_blocks.get_mut(&message_id).map(|seen| {
            let from = *seen;
            *seen += blocks.len();
            from
        });
        for (i, block) in blocks.iter().enumerate() {
            let item_id = match streamed_from {
                Some(from) => format!("{message_id}:{}", from + i),
                None if blocks.len() > 1 => format!("{line_id}:{i}"),
                None => line_id.clone(),
            };
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
                    if name == "TodoWrite" && sub.is_none() {
                        out.push(Emitted { subagent_id: None, body: EventBody::PlanUpdated { steps: todo_steps(&input), explanation: None } });
                    }
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
                            model: None,
                            kind: None,
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
            let (name, input) = self.calls.remove(&call_id).unwrap_or_default();
            let diff = self.file_diff(&name, &raw["tool_use_result"]);
            if sub.is_none() && status == ToolStatus::Ok && self.task_list(&name, &input, &output) {
                let steps = self.tasks.iter().map(|(_, s)| s.clone()).collect();
                out.push(Emitted { subagent_id: None, body: EventBody::PlanUpdated { steps, explanation: None } });
            }
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
    /// `TaskCreate` / `TaskUpdate` 成功了：改记着的清单。改了返回真。
    fn task_list(&mut self, name: &str, input: &Value, output: &str) -> bool {
        match name {
            "TaskCreate" => {
                // 结果是「Task #3 created successfully: …」。
                let Some(id) = output.split_once("Task #").and_then(|(_, rest)| rest.split_whitespace().next()) else { return false };
                let text = input["subject"].as_str().unwrap_or("").trim().to_string();
                self.tasks.push((id.to_string(), PlanStep { text, status: PlanStatus::Pending }));
                true
            }
            "TaskUpdate" => {
                let id = match &input["taskId"] {
                    Value::String(s) => s.trim().trim_start_matches('#').to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => return false,
                };
                let Some(i) = self.tasks.iter().position(|(t, _)| *t == id) else { return false };
                if input["status"] == "deleted" {
                    self.tasks.remove(i);
                    return true;
                }
                let step = &mut self.tasks[i].1;
                if let Some(subject) = input["subject"].as_str().filter(|s| !s.trim().is_empty()) {
                    step.text = subject.trim().to_string();
                }
                if let Some(status) = input["status"].as_str() {
                    step.status = plan_status(status);
                }
                true
            }
            _ => false,
        }
    }

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
        // 窗口回合末才知道：补报一次「用了 / 窗口」。
        if context_window.is_some() && context_window != self.context_window {
            self.context_window = context_window;
            if let Some(used) = self.context_used {
                out.push(super::cli_context("Claude Code", used, context_window, false).into());
            }
        }
        out.push(
            EventBody::UsageUpdated {
                usage: TokenUsage {
                    input: n("input_tokens"),
                    output: n("output_tokens"),
                    cache_read: n("cache_read_input_tokens"),
                    cache_write: n("cache_creation_input_tokens"),
                    reasoning: u["output_tokens_details"]["thinking_tokens"].as_u64(),
                    web_searches: None,
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
                model: None,
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

    /// `--include-partial-messages` 的增量。item id 是「消息 id:第几块」，和完成的那条（`assistant`）对得上。
    fn stream_event(&mut self, raw: &Value, sub: &Option<String>) -> Vec<Emitted> {
        let e = &raw["event"];
        match e["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.streaming_message = e["message"]["id"].as_str().map(str::to_string);
                if let Some(id) = &self.streaming_message {
                    self.streamed_blocks.entry(id.clone()).or_insert(0);
                }
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
                    // A1：项目 .mcp.json 配了、Claude Code 却没连上的（没批过的不会在非交互里问）。
                    if let Some(cwd) = raw["cwd"].as_str() {
                        let reported: Vec<(String, String)> = raw["mcp_servers"]
                            .as_array()
                            .map(|m| {
                                m.iter()
                                    .map(|s| (s["name"].as_str().unwrap_or("").to_string(), s["status"].as_str().unwrap_or("").to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let missing = crate::gateway::project_mcp::claude_code_missing(std::path::Path::new(cwd), &reported);
                        if !missing.is_empty() {
                            out.push(
                                EventBody::Notice {
                                    level: NoticeLevel::Warn,
                                    text: crate::gateway::project_mcp::notice("Claude Code", "claude-code", &missing),
                                }
                                .into(),
                            );
                        }
                    }
                    self.turn_start(&mut out);
                }
                // 压了一次（v1.1.7）：`post_tokens` 只算对话，不拿来画环；用量等下一条回答再报。
                "compact_boundary" => {
                    let m = &raw["compact_metadata"];
                    let focus = m["user_context"].as_str().map(str::trim).filter(|f| !f.is_empty()).map(|f| format!("，重点：{}", short(f, 60))).unwrap_or_default();
                    out.push(super::cli_compacted("Claude Code", m["trigger"].as_str().map(|t| t == "manual"), m["pre_tokens"].as_u64(), None, &focus).into());
                    self.context_used = None;
                }
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
                    Some("can_use_tool") if request["tool_name"] == "AskUserQuestion" => {
                        // 模型要问站主：ASK 卡片。回答由引擎那一层放进这次审批的 `updatedInput`。
                        let questions = request["input"]["questions"].as_array().cloned().unwrap_or_default();
                        out.push(super::ask_event(raw["request_id"].as_str().unwrap_or("").into(), &questions, true, |q| q["multiSelect"] == true).into());
                    }
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
                                reason: None,
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

/// Claude Code 清单里的状态。
fn plan_status(s: &str) -> PlanStatus {
    match s {
        "completed" => PlanStatus::Completed,
        "in_progress" => PlanStatus::InProgress,
        _ => PlanStatus::Pending,
    }
}

/// `TodoWrite` 的整份清单（`todos[].content`、`status`）。
fn todo_steps(input: &Value) -> Vec<PlanStep> {
    input["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| PlanStep { text: t["content"].as_str().unwrap_or("").trim().to_string(), status: plan_status(t["status"].as_str().unwrap_or("")) })
        .filter(|s| !s.text.is_empty())
        .collect()
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
        assert_eq!(titles, ["读 notes.txt", "改 notes.txt", "跑：Write done to stamp.txt"], "Bash 带 description 时拿它当标题（v1.1.11）");
        let edit = events.iter().find_map(|e| match &e.body {
            EventBody::ToolCallCompleted { diff, .. } if !diff.is_empty() => Some(diff[0].clone()),
            _ => None,
        });
        assert_eq!(edit.map(|d| (d.path, d.additions)), Some(("notes.txt".into(), Some(1))));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::UsageUpdated { cost, .. } if cost.source == CostSource::Estimated)));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::QuotaUpdated { provider, .. } if provider == "claude-code")));
    }

    #[test]
    fn streamed_pieces_and_the_finished_message_share_ids() {
        // `--include-partial-messages`：增量和完成的那条要是同一个 item id，界面才不会显示两遍（2026-11-12 本地验收查到）。
        let mut c = ClaudeCodeConverter::new(None);
        let feed = |c: &mut ClaudeCodeConverter, v: Value| c.feed(&v);
        let ev = |t: &str, index: u64, delta: Value| json!({"type": "stream_event", "event": {"type": t, "index": index, "delta": delta}});
        feed(&mut c, json!({"type": "stream_event", "event": {"type": "message_start", "message": {"id": "msg_1"}}}));
        let mut ids: Vec<(String, String)> = Vec::new();
        let mut take = |out: Vec<Emitted>| {
            for e in out {
                match e.body {
                    EventBody::ReasoningDelta { item_id, .. } => ids.push(("reasoning.delta".into(), item_id)),
                    EventBody::ReasoningCompleted { item_id, .. } => ids.push(("reasoning.completed".into(), item_id)),
                    EventBody::MessageDelta { item_id, .. } => ids.push(("message.delta".into(), item_id)),
                    EventBody::MessageCompleted { item_id, .. } => ids.push(("message.completed".into(), item_id)),
                    _ => {}
                }
            }
        };
        take(feed(&mut c, ev("content_block_delta", 0, json!({"type": "thinking_delta", "thinking": "想"}))));
        take(feed(&mut c, json!({"type": "assistant", "uuid": "u1", "message": {"id": "msg_1", "content": [{"type": "thinking", "thinking": "想"}]}})));
        take(feed(&mut c, ev("content_block_delta", 1, json!({"type": "text_delta", "text": "好"}))));
        take(feed(&mut c, json!({"type": "assistant", "uuid": "u2", "message": {"id": "msg_1", "content": [{"type": "text", "text": "好的"}]}})));
        take(feed(&mut c, json!({"type": "assistant", "uuid": "u3", "message": {"id": "msg_1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}})));
        // 没流式过的消息照旧用行的 uuid。
        take(feed(&mut c, json!({"type": "assistant", "uuid": "u4", "message": {"id": "msg_2", "content": [{"type": "text", "text": "完"}]}})));
        assert_eq!(
            ids,
            [
                ("reasoning.delta".to_string(), "msg_1:0".to_string()),
                ("reasoning.completed".into(), "msg_1:0".into()),
                ("message.delta".into(), "msg_1:1".into()),
                ("message.completed".into(), "msg_1:1".into()),
                ("message.completed".into(), "u4".into()),
            ]
        );
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

    #[test]
    fn todo_write_and_task_tools_become_the_plan() {
        let mut c = ClaudeCodeConverter::new(Some("/ws".into()));
        let plan = |out: &[Emitted]| {
            out.iter().find_map(|e| match &e.body {
                EventBody::PlanUpdated { steps, .. } => Some(steps.iter().map(|s| format!("{}:{:?}", s.text, s.status)).collect::<Vec<_>>()),
                _ => None,
            })
        };
        let use_tool = |id: &str, name: &str, input: Value, parent: Value| json!({"type": "assistant", "uuid": format!("u-{id}"), "parent_tool_use_id": parent, "message": {"content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}});
        let result = |id: &str, text: &str| json!({"type": "user", "uuid": format!("r-{id}"), "parent_tool_use_id": null, "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": text}]}});
        // 旧的 TodoWrite：每次整份。
        let out = c.feed(&use_tool("w1", "TodoWrite", json!({"todos": [{"content": "读代码", "status": "completed", "activeForm": "读代码中"}, {"content": "改", "status": "in_progress"}]}), Value::Null));
        assert_eq!(plan(&out), Some(vec!["读代码:Completed".to_string(), "改:InProgress".into()]));
        assert!(out.iter().any(|e| matches!(&e.body, EventBody::ToolCallStarted { title, .. } if title == "更新任务清单")));
        // 2.1 起的 TaskCreate / TaskUpdate：编号在结果里，Peck 拼成整份。
        c.feed(&use_tool("c1", "TaskCreate", json!({"subject": "加测试", "description": "…"}), Value::Null));
        assert_eq!(plan(&c.feed(&result("c1", "Task #1 created successfully: 加测试"))), Some(vec!["加测试:Pending".to_string()]));
        c.feed(&use_tool("c2", "TaskCreate", json!({"subject": "跑一遍"}), Value::Null));
        c.feed(&result("c2", "Task #2 created successfully: 跑一遍"));
        c.feed(&use_tool("u1", "TaskUpdate", json!({"taskId": "1", "status": "completed"}), Value::Null));
        assert_eq!(plan(&c.feed(&result("u1", "Updated task #1 status"))), Some(vec!["加测试:Completed".to_string(), "跑一遍:Pending".into()]));
        c.feed(&use_tool("u2", "TaskUpdate", json!({"taskId": "2", "status": "deleted"}), Value::Null));
        assert_eq!(plan(&c.feed(&result("u2", "Updated task #2 deleted"))), Some(vec!["加测试:Completed".to_string()]));
        // 子任务里的清单不盖掉主线的。
        let inner = c.feed(&use_tool("w2", "TodoWrite", json!({"todos": [{"content": "子任务自己的", "status": "pending"}]}), json!("task1")));
        assert_eq!(plan(&inner), None);
    }

    /// 上下文用量（v1.1.7）：主线每条回答报一次（同一个消息 id 只算一次、子代理里的不算），窗口回合末补上；压缩记一行。
    #[test]
    fn context_usage_and_compaction_are_reported() {
        let mut c = ClaudeCodeConverter::new(None);
        let msg = |id: &str, parent: Value, input: u64| json!({"type": "assistant", "uuid": format!("u-{id}-{input}"), "parent_tool_use_id": parent, "message": {"id": id, "content": [{"type": "text", "text": "x"}], "usage": {"input_tokens": input, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 1000}}});
        let used = |out: &[Emitted]| out.iter().find_map(|e| match &e.body { EventBody::ContextUpdated { used, window, managed_by, .. } => Some((*used, *window, managed_by.clone())), _ => None });
        assert_eq!(used(&c.feed(&msg("m1", Value::Null, 5))), Some((1105, None, Some("Claude Code".into()))));
        assert_eq!(used(&c.feed(&msg("m1", Value::Null, 5))), None, "同一个消息 id 只算一次");
        assert_eq!(used(&c.feed(&msg("m2", json!("task1"), 9))), None, "子代理里的不算");
        let end = c.feed(&json!({"type": "result", "subtype": "success", "usage": {}, "modelUsage": {"claude": {"contextWindow": 200000}}}));
        assert_eq!(used(&end), Some((1105, Some(200000), Some("Claude Code".into()))));
        let boundary = c.feed(&json!({"type": "system", "subtype": "compact_boundary", "compact_metadata": {"trigger": "manual", "pre_tokens": 150000, "user_context": "数据库迁移"}}));
        assert!(matches!(&boundary[0].body, EventBody::ContextCompacted { before: 150000, after: 0, text, .. } if text.contains("你让它压的") && text.contains("重点：数据库迁移")));
    }

}
