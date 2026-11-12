//! ACP（Agent Client Protocol，stdio JSON-RPC）→ 统一事件。Grok（`grok agent stdio`）和
//! Antigravity（`agy acp`）共用，引擎 id 不同。
//!
//! - 过程都在 `session/update` 里（`sessionUpdate`：`agent_message_chunk`、`agent_thought_chunk`、
//!   `tool_call`、`tool_call_update`、`plan`……）。ACP 的文字没有「段落完了」的信号：碰到别的事件
//!   或一轮结束，就把攒着的文字收成一条 `message.completed`。
//! - 审批是引擎发来的请求 `session/request_permission`，回答就是这个请求的结果。
//! - 一轮的结束和用量在 `session/prompt` 的回应里（`stopReason`；用量在 `usage` 或 `_meta.usage`）。
//!   `session/new` 的回应带会话号和模型。转换器按回应的内容认，不用知道请求号。
//! - Grok 的 `_x.ai/…` 扩展通知不出事件。

use super::{count_lines, relative, short, Emitted, LineConverter};
use crate::gateway::caps::AgentCaps;
use crate::gateway::event::{
    ApprovalDecision, Cost, CostSource, EventBody, FileChange, FileDiff, PlanStatus, PlanStep,
    TokenUsage, ToolKind, ToolStatus, TurnEndReason, UsageScope,
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub struct AcpConverter {
    engine: String,
    cwd: Option<String>,
    turn: u32,
    message: Option<(String, String)>,
    thought: Option<(String, String)>,
    pieces: u32,
    started_calls: HashSet<String>,
    finished_calls: HashSet<String>,
    model: Option<String>,
}

impl AcpConverter {
    pub fn new(engine: &str, cwd: Option<String>) -> Self {
        Self {
            engine: engine.into(),
            cwd,
            turn: 0,
            message: None,
            thought: None,
            pieces: 0,
            started_calls: HashSet::new(),
            finished_calls: HashSet::new(),
            model: None,
        }
    }

    /// 审批在统一事件里的 id：`<引擎>-<JSON-RPC id>`。
    pub fn request_id(engine: &str, rpc_id: &Value) -> String {
        match rpc_id {
            Value::String(s) => format!("{engine}-{s}"),
            other => format!("{engine}-{other}"),
        }
    }

    /// 统一的审批选择 → 引擎给的选项里的 `optionId`。没有「总是」的退成「这次」。
    pub fn option_for(decision: ApprovalDecision, options: &Value) -> Option<String> {
        let list = options.as_array()?;
        let find = |kinds: &[&str]| {
            kinds.iter().find_map(|k| {
                list.iter()
                    .find(|o| o["kind"] == *k)
                    .and_then(|o| o["optionId"].as_str().map(str::to_string))
            })
        };
        match decision {
            ApprovalDecision::AllowOnce => find(&["allow_once", "allow_always"]),
            ApprovalDecision::AllowAlways => find(&["allow_always", "allow_once"]),
            ApprovalDecision::DenyOnce => find(&["reject_once", "reject_always"]),
            ApprovalDecision::DenyAlways => find(&["reject_always", "reject_once"]),
        }
    }

    /// 回答一次 `session/request_permission`（选不出选项就算取消）。
    pub fn permission_reply(rpc_id: &Value, decision: ApprovalDecision, options: &Value) -> Value {
        let outcome = match Self::option_for(decision, options) {
            Some(id) => json!({"outcome": "selected", "optionId": id}),
            None => json!({"outcome": "cancelled"}),
        };
        json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"outcome": outcome}})
    }

    fn caps(&self) -> AgentCaps {
        AgentCaps::for_engine(&self.engine)
    }

    fn rel(&self, path: &str) -> String {
        relative(path, self.cwd.as_deref())
    }

    fn turn_id(&self) -> String {
        format!("turn-{}", self.turn.max(1))
    }

    /// 一轮开始（引擎那一层发 `session/prompt` 时调）。ACP 自己没有这个事件。
    pub fn start_turn(&mut self) -> Vec<EventBody> {
        self.turn += 1;
        vec![EventBody::TurnStarted { turn_id: self.turn_id() }]
    }

    fn flush(&mut self) -> Vec<EventBody> {
        let mut out = Vec::new();
        if let Some((item_id, text)) = self.thought.take() {
            out.push(EventBody::ReasoningCompleted { item_id, text, opaque: None });
        }
        if let Some((item_id, text)) = self.message.take() {
            out.push(EventBody::MessageCompleted { item_id, text });
        }
        out
    }

    fn chunk_text(update: &Value) -> String {
        update["content"]["text"].as_str().unwrap_or("").to_string()
    }

    /// 工具的名字、种类、给人看的标题。
    fn describe(&self, u: &Value) -> (String, ToolKind, String) {
        let meta = &u["_meta"]["x.ai/tool"];
        let name = u["name"]
            .as_str()
            .or(meta["name"].as_str())
            .or(u["title"].as_str())
            .unwrap_or("")
            .to_string();
        let input = if meta["input"].is_object() { &meta["input"] } else { &u["rawInput"] };
        let kind_word = u["kind"].as_str().or(meta["kind"].as_str()).unwrap_or("other");
        let path = input["path"]
            .as_str()
            .or(input["Path"].as_str())
            .or(input["target_file"].as_str())
            .or(input["file_path"].as_str())
            .or(u["locations"][0]["path"].as_str())
            .map(|p| self.rel(p))
            .unwrap_or_default();
        let command = input["command"].as_str().or(input["CommandLine"].as_str()).unwrap_or("");
        let (kind, title) = match kind_word {
            "read" => (ToolKind::Read, format!("读 {path}")),
            "edit" | "delete" | "move" => (ToolKind::Edit, if path.is_empty() { "改文件".into() } else { format!("改 {path}") }),
            "execute" => (ToolKind::Run, format!("跑 {}", short(command, 80))),
            "search" => {
                let q = input["pattern"].as_str().or(input["query"].as_str()).or(input["Query"].as_str()).unwrap_or(&path);
                (ToolKind::Search, format!("搜 {}", short(q, 60)))
            }
            "fetch" => (ToolKind::Fetch, format!("取 {}", short(input["url"].as_str().or(input["Url"].as_str()).unwrap_or(""), 60))),
            "think" => (ToolKind::Other, "想一想".into()),
            _ => {
                // Grok 经通用的 use_tool 调 MCP 工具：显示真正的工具名（`peck__ask_user` 是问站主）。
                let inner = input["tool_name"].as_str().or(u["title"].as_str().filter(|t| t.contains("__")));
                match inner {
                    Some(t) if t.ends_with("ask_user") => (ToolKind::Mcp, "问站主".into()),
                    Some(t) => (ToolKind::Mcp, format!("用 {}", short(t, 60))),
                    None => (ToolKind::Other, short(u["title"].as_str().unwrap_or(&name), 60)),
                }
            }
        };
        (name, kind, title)
    }

    fn tool_started(&mut self, u: &Value) -> Vec<EventBody> {
        let Some(id) = u["toolCallId"].as_str() else { return vec![] };
        if !self.started_calls.insert(id.to_string()) {
            return vec![];
        }
        let mut out = self.flush();
        let (name, kind, title) = self.describe(u);
        let input = if u["_meta"]["x.ai/tool"]["input"].is_object() {
            u["_meta"]["x.ai/tool"]["input"].clone()
        } else {
            u["rawInput"].clone()
        };
        out.push(EventBody::ToolCallStarted {
            call_id: id.into(),
            name,
            kind,
            title,
            input: relativize(input, self.cwd.as_deref()),
        });
        out
    }

    fn tool_update(&mut self, u: &Value) -> Vec<EventBody> {
        let Some(id) = u["toolCallId"].as_str() else { return vec![] };
        let mut out = Vec::new();
        if !self.started_calls.contains(id) {
            out.extend(self.tool_started(u));
        }
        let status = match u["status"].as_str() {
            Some("completed") => ToolStatus::Ok,
            Some("failed") => ToolStatus::Failed,
            _ => return out,
        };
        if !self.finished_calls.insert(id.to_string()) {
            return out;
        }
        let raw_output = &u["rawOutput"];
        let exit_code = raw_output["exit_code"].as_i64().map(|c| c as i32);
        let text: String = u["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "content")
            .filter_map(|c| c["content"]["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let output = raw_output["output_for_prompt"]
            .as_str()
            .or(raw_output["output"].as_str())
            .map(str::to_string)
            .or_else(|| (!text.trim().is_empty()).then_some(text));
        let diff: Vec<FileDiff> = u["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "diff")
            .map(|c| file_diff(&self.rel(c["path"].as_str().unwrap_or("")), c["oldText"].as_str(), c["newText"].as_str().unwrap_or("")))
            .collect();
        let status = if status == ToolStatus::Ok && exit_code.is_some_and(|c| c != 0) { ToolStatus::Failed } else { status };
        out.push(EventBody::ToolCallCompleted {
            call_id: id.into(),
            status,
            output,
            exit_code,
            diff,
            duration_ms: None,
        });
        out
    }

    fn session_update(&mut self, u: &Value) -> Vec<EventBody> {
        match u["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" => {
                let text = Self::chunk_text(u);
                let mut out = Vec::new();
                if let Some((item_id, t)) = self.thought.take() {
                    out.push(EventBody::ReasoningCompleted { item_id, text: t, opaque: None });
                }
                if self.message.is_none() {
                    self.pieces += 1;
                    self.message = Some((format!("{}-msg-{}", self.engine, self.pieces), String::new()));
                }
                let (item_id, all) = self.message.as_mut().expect("刚放进去");
                all.push_str(&text);
                if !text.is_empty() {
                    out.push(EventBody::MessageDelta { item_id: item_id.clone(), text });
                }
                out
            }
            "agent_thought_chunk" => {
                let text = Self::chunk_text(u);
                let mut out = Vec::new();
                if let Some((item_id, t)) = self.message.take() {
                    out.push(EventBody::MessageCompleted { item_id, text: t });
                }
                if self.thought.is_none() {
                    self.pieces += 1;
                    self.thought = Some((format!("{}-thought-{}", self.engine, self.pieces), String::new()));
                }
                let (item_id, all) = self.thought.as_mut().expect("刚放进去");
                all.push_str(&text);
                if !text.is_empty() {
                    out.push(EventBody::ReasoningDelta { item_id: item_id.clone(), text });
                }
                out
            }
            "tool_call" => {
                let mut out = self.tool_started(u);
                // 有的引擎（Antigravity）一开始就带 completed。
                if matches!(u["status"].as_str(), Some("completed") | Some("failed")) {
                    out.extend(self.tool_update(u));
                }
                out
            }
            "tool_call_update" => self.tool_update(u),
            "plan" => {
                let mut out = self.flush();
                let steps = u["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| PlanStep {
                        text: e["content"].as_str().unwrap_or("").into(),
                        status: match e["status"].as_str() {
                            Some("completed") => PlanStatus::Completed,
                            Some("in_progress") => PlanStatus::InProgress,
                            _ => PlanStatus::Pending,
                        },
                    })
                    .collect();
                out.push(EventBody::PlanUpdated { steps, explanation: None });
                out
            }
            "config_option_update" => {
                let model = u["configOptions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|o| o["id"] == "model")
                    .and_then(|o| o["currentValue"].as_str())
                    .map(str::to_string);
                if model.is_some() && model != self.model {
                    self.model = model.clone();
                    return vec![EventBody::SessionConfigChanged { model, permission: None, effort: None }];
                }
                vec![]
            }
            // 用户自己那一句、可用命令、模式、标题：不出事件。
            _ => vec![],
        }
    }

    /// 一轮的回应（带 `stopReason`）→ 用量和回合结束。
    fn prompt_done(&mut self, result: &Value) -> Vec<EventBody> {
        let mut out = self.flush();
        let u = if result["usage"].is_object() { &result["usage"] } else { &result["_meta"]["usage"] };
        if u.is_object() {
            let n = |k: &str| u[k].as_u64();
            let read = n("cachedReadTokens");
            let write = n("cachedWriteTokens").or(n("cacheCreationTokens"));
            let ticks = u["costUsdTicks"].as_f64();
            out.push(EventBody::UsageUpdated {
                usage: TokenUsage {
                    // ACP 的 inputTokens 有的含缓存（Grok），有的不含（Antigravity：缓存读另报、大于输入）。
                    input: n("inputTokens").map(|i| match read {
                        Some(r) if r <= i && self.engine == "grok" => i - r - write.unwrap_or(0).min(i - r),
                        _ => i,
                    }),
                    output: n("outputTokens"),
                    cache_read: read,
                    cache_write: write,
                    reasoning: n("reasoningTokens").or(n("thoughtTokens")),
                    context_used: None,
                    context_window: None,
                },
                cost: match ticks {
                    // Grok 的花费单位：1 美元 = 10¹⁰ ticks（CLI 自己估的）。
                    Some(t) => Cost { usd: Some(t / 1e10), source: CostSource::Estimated },
                    None => Cost::unknown(),
                },
                scope: UsageScope::Turn,
            });
        }
        let reason = match result["stopReason"].as_str() {
            Some("end_turn") => TurnEndReason::Done,
            Some("max_tokens") | Some("max_turn_requests") => TurnEndReason::Limit,
            Some("refusal") => TurnEndReason::Refused,
            Some("cancelled") => TurnEndReason::Interrupted,
            _ => TurnEndReason::Error,
        };
        out.push(EventBody::TurnCompleted {
            turn_id: self.turn_id(),
            reason,
            error: None,
        });
        out
    }

    pub(crate) fn convert(&mut self, raw: &Value) -> Vec<EventBody> {
        let method = raw["method"].as_str();
        // 引擎发来的请求：审批。
        if let (Some(m), Some(id)) = (method, raw.get("id")) {
            if m == "session/request_permission" {
                let p = &raw["params"];
                let tc = &p["toolCall"];
                let mut out = self.flush();
                let (_, kind, title) = self.describe(tc);
                let input = if tc["_meta"]["x.ai/tool"]["input"].is_object() { &tc["_meta"]["x.ai/tool"]["input"] } else { &tc["rawInput"] };
                let mut options = Vec::new();
                for d in [ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce, ApprovalDecision::DenyAlways] {
                    let want = match d {
                        ApprovalDecision::AllowOnce => "allow_once",
                        ApprovalDecision::AllowAlways => "allow_always",
                        ApprovalDecision::DenyOnce => "reject_once",
                        ApprovalDecision::DenyAlways => "reject_always",
                    };
                    if p["options"].as_array().is_some_and(|o| o.iter().any(|x| x["kind"] == want)) {
                        options.push(d);
                    }
                }
                out.push(EventBody::ApprovalRequested {
                    request_id: Self::request_id(&self.engine, id),
                    call_id: tc["toolCallId"].as_str().map(str::to_string),
                    kind,
                    title,
                    detail: input["command"].as_str().or(input["CommandLine"].as_str()).map(str::to_string),
                    high_risk: false,
                    options,
                });
                return out;
            }
            return vec![EventBody::Raw { source: self.engine.clone(), kind: Some(m.into()) }];
        }
        match method {
            Some("session/update") => self.session_update(&raw["params"]["update"]),
            Some(m) if m.starts_with("_") => vec![],
            Some(m) => vec![EventBody::Raw { source: self.engine.clone(), kind: Some(m.into()) }],
            None => {
                let result = &raw["result"];
                if result["stopReason"].is_string() {
                    return self.prompt_done(result);
                }
                if let Some(session) = result["sessionId"].as_str() {
                    self.model = result["models"]["currentModelId"].as_str().map(str::to_string);
                    return vec![EventBody::SessionStarted {
                        engine: self.engine.clone(),
                        model: self.model.clone(),
                        caps: self.caps(),
                        tools: vec![],
                        mcp: vec![],
                        session_ref: Some(session.into()),
                        version: None,
                    }];
                }
                if let Some(message) = raw["error"]["message"].as_str() {
                    let mut out = self.flush();
                    out.push(EventBody::Error { message: format!("{}：{}", self.engine, short(message, 200)), fatal: false });
                    return out;
                }
                vec![]
            }
        }
    }
}

/// 绝对路径换成项目里的相对路径（给人看的输入）。
fn relativize(v: Value, root: Option<&str>) -> Value {
    match v {
        Value::String(s) => Value::String(relative(&s, root)),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, relativize(v, root))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().map(|v| relativize(v, root)).collect()),
        other => other,
    }
}

/// ACP 给的是改之前和改之后的全文：去掉相同的开头和结尾，中间的算增删，拼一段简单的 unified diff。
fn file_diff(path: &str, old: Option<&str>, new: &str) -> FileDiff {
    let old_lines: Vec<&str> = old.unwrap_or("").lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let prefix = old_lines.iter().zip(&new_lines).take_while(|(a, b)| a == b).count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let removed = &old_lines[prefix..old_lines.len() - suffix];
    let added = &new_lines[prefix..new_lines.len() - suffix];
    let mut patch = format!("--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n", prefix + 1, removed.len(), prefix + 1, added.len());
    for l in removed {
        patch.push_str(&format!("-{l}\n"));
    }
    for l in added {
        patch.push_str(&format!("+{l}\n"));
    }
    let (additions, deletions) = count_lines(&patch);
    FileDiff {
        path: path.into(),
        change: if old.is_none() { FileChange::Added } else { FileChange::Modified },
        old_path: None,
        additions: Some(additions),
        deletions: Some(deletions),
        patch: Some(patch),
    }
}

impl LineConverter for AcpConverter {
    fn feed(&mut self, raw: &Value) -> Vec<Emitted> {
        self.convert(raw).into_iter().map(Into::into).collect()
    }

    fn finish(&mut self) -> Vec<Emitted> {
        self.flush().into_iter().map(Into::into).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::fixtures;

    fn run(engine: &str, name: &str) -> Vec<EventBody> {
        let mut c = AcpConverter::new(engine, Some("{{WORKSPACE}}".into()));
        let raw = fixtures::read(name, "raw.jsonl");
        // 夹具里 Peck 发 session/prompt 的那一刻没有原件：录的时候设完模型和强度（请求 3、4）
        // 才发提示，就在第 4 个回应之后补一个回合开始（和引擎一样）。
        let mut events = Vec::new();
        for line in raw.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line).unwrap();
            events.extend(c.feed(&v));
            if v.get("method").is_none() && v["id"] == json!(4) {
                events.extend(c.start_turn().into_iter().map(Emitted::from));
            }
        }
        events.extend(c.finish());
        fixtures::check(name, &events);
        events.into_iter().map(|e| e.body).collect()
    }

    #[test]
    fn grok_small_task_fixture() {
        let bodies = run("grok", "grok-small-task");
        assert!(matches!(bodies.first(), Some(EventBody::SessionStarted { .. })));
        assert!(matches!(bodies.last(), Some(EventBody::TurnCompleted { reason: TurnEndReason::Done, .. })));
        let titles: Vec<&str> = bodies.iter().filter_map(|b| match b { EventBody::ToolCallStarted { title, .. } => Some(title.as_str()), _ => None }).collect();
        assert_eq!(titles, vec!["读 notes.txt", "跑 echo grok > stamp.txt"]);
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ApprovalRequested { options, .. } if options.len() >= 3)));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::UsageUpdated { cost, .. } if cost.source == CostSource::Estimated)));
        assert!(!bodies.iter().any(|b| matches!(b, EventBody::Raw { .. })));
    }

    #[test]
    fn antigravity_small_task_fixture() {
        let bodies = run("antigravity", "antigravity-small-task");
        let titles: Vec<&str> = bodies.iter().filter_map(|b| match b { EventBody::ToolCallStarted { title, .. } => Some(title.as_str()), _ => None }).collect();
        assert_eq!(titles, vec!["读 notes.txt", "跑 echo agy > stamp.txt"]);
        assert!(bodies.iter().any(|b| matches!(b, EventBody::ApprovalRequested { options, .. } if options == &vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce])));
        assert!(bodies.iter().any(|b| matches!(b, EventBody::MessageCompleted { .. })));
        assert!(matches!(bodies.last(), Some(EventBody::TurnCompleted { reason: TurnEndReason::Done, .. })));
    }

    #[test]
    fn mcp_tools_called_through_use_tool_show_their_real_name() {
        let c = AcpConverter::new("grok", None);
        let u = json!({"toolCallId": "c1", "title": "use_tool", "rawInput": {"tool_name": "peck__ask_user", "tool_input": {"question": "?"}}});
        let (_, kind, title) = c.describe(&u);
        assert_eq!((kind, title.as_str()), (ToolKind::Mcp, "问站主"));
        let perm = json!({"toolCallId": "c1", "title": "peck__ask_user", "kind": "other"});
        assert_eq!(c.describe(&perm).2, "问站主");
    }

    #[test]
    fn diffs_and_options_map_over() {
        let d = file_diff("a.txt", Some("x\ny\nz\n"), "x\nY\nz\n");
        assert_eq!((d.additions, d.deletions), (Some(1), Some(1)));
        assert!(d.patch.unwrap().contains("-y\n+Y\n"));
        let added = file_diff("b.txt", None, "new\n");
        assert_eq!((added.change, added.additions), (FileChange::Added, Some(1)));
        let opts = json!([{"optionId": "allow", "kind": "allow_once"}, {"optionId": "deny", "kind": "reject_once"}]);
        assert_eq!(AcpConverter::option_for(ApprovalDecision::AllowAlways, &opts).as_deref(), Some("allow"));
        assert_eq!(AcpConverter::option_for(ApprovalDecision::DenyAlways, &opts).as_deref(), Some("deny"));
        let reply = AcpConverter::permission_reply(&json!(3), ApprovalDecision::DenyOnce, &opts);
        assert_eq!(reply["result"]["outcome"], json!({"outcome": "selected", "optionId": "deny"}));
    }
}
