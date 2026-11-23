//! OpenAI Responses（流式 SSE）：统一的请求 → body，流 → 增量和整条回答。
//!
//! 不让 OpenAI 存对话（`store: false`），所以思考要带着密文回来：请求里要
//! `include: ["reasoning.encrypted_content"]`，回放时把 `reasoning` 项（摘要 + 密文）和
//! `function_call` 项原样放回 `input`。回放的项不带 `id`（不存的时候 OpenAI 找不到这些 id）。
//!
//! 联网搜索（v1.1.1）：工具里加 `{"type": "web_search"}`，`include` 要上来源（`web_search_call.action.sources`）；
//! 回答里的 `web_search_call` 项存成 `Part::ServerTool`，同一来源原样放回（也不带 `id`），一次 `search` 记一次。
//!
//! 百炼（v1.1.2）也走这条线协议（[`Dialect::Qwen`]）：思考开关是顶层的 `enable_thinking`、强度 `reasoning.effort`，
//! 没有 `include`（文档没写，不发），思考流在 `response.reasoning_text.delta`；联网搜索一样是 `web_search`。

use super::{minus, priced, system_text, WireStream};
use crate::gateway::catalog::{ModelSpec, Thinking};
use crate::gateway::event::{OpaqueReasoning, Origin, TokenUsage};
use crate::gateway::search::{self, Native, Source};
use crate::gateway::{Message, ModelEvent, ModelRequest, ModelResponse, Part, Role, StopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 同一条线协议的两种方言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    OpenAi,
    /// 百炼（DashScope 的 OpenAI 兼容 Responses）。
    Qwen,
}

impl Dialect {
    pub fn from_catalog(dialect: Option<&str>) -> Self {
        match dialect {
            Some("qwen") => Self::Qwen,
            _ => Self::OpenAi,
        }
    }
}

/// 统一的请求 → Responses 的 body。
pub fn encode(request: &ModelRequest, model: &ModelSpec, origin: &Origin, dialect: Dialect) -> Value {
    let mut input = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::User => user_items(message, &mut input),
            Role::Assistant => assistant_items(message, origin, &mut input),
        }
    }
    let mut body = json!({
        "model": model.api_model,
        "input": input,
        "stream": true,
        "store": false,
        "max_output_tokens": request.max_output_tokens,
    });
    if dialect == Dialect::OpenAi {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    let system = system_text(request);
    if !system.is_empty() {
        body["instructions"] = json!(system);
    }
    let mut tools: Vec<Value> = request
        .tools
        .iter()
        .map(|t| json!({"type": "function", "name": t.name, "description": t.description, "parameters": t.input_schema, "strict": false}))
        .collect();
    if request.web_search && search::native(model) == Some(Native::Responses) {
        tools.push(json!({"type": "web_search"}));
        if dialect == Dialect::OpenAi {
            body["include"] = json!(["reasoning.encrypted_content", "web_search_call.action.sources"]);
        }
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    match (dialect, model.thinking, request.thinking) {
        (Dialect::OpenAi, Thinking::Always, _) | (Dialect::OpenAi, Thinking::Switch, true) => {
            let mut reasoning = json!({"summary": "auto"});
            if let Some(effort) = &request.effort {
                reasoning["effort"] = json!(effort);
            }
            body["reasoning"] = reasoning;
        }
        (Dialect::OpenAi, Thinking::Switch, false) => body["reasoning"] = json!({"effort": "none"}),
        // 百炼：开关用 enable_thinking（有的模型不认 effort 的 none），开着时有强度就带 reasoning.effort。
        (Dialect::Qwen, Thinking::Never, _) | (Dialect::OpenAi, Thinking::Never, _) => {}
        (Dialect::Qwen, thinking, asked) => {
            let on = thinking == Thinking::Always || asked;
            if thinking == Thinking::Switch {
                body["enable_thinking"] = json!(on);
            }
            if let (true, Some(effort)) = (on, &request.effort) {
                body["reasoning"] = json!({"effort": effort});
            }
        }
    }
    body
}

fn user_items(message: &Message, input: &mut Vec<Value>) {
    let mut texts = Vec::new();
    for part in &message.parts {
        match part {
            Part::ToolResult {
                call_id, output, ..
            } => input.push(json!({"type": "function_call_output", "call_id": call_id, "output": output})),
            Part::Text { text } => texts.push(json!({"type": "input_text", "text": text})),
            Part::Image { media_type, data } => {
                texts.push(json!({"type": "input_image", "image_url": format!("data:{media_type};base64,{data}")}))
            }
            Part::Reasoning { .. } | Part::ToolCall { .. } | Part::ServerTool { .. } => {}
        }
    }
    if !texts.is_empty() {
        input.push(json!({"type": "message", "role": "user", "content": texts}));
    }
}

fn assistant_items(message: &Message, origin: &Origin, input: &mut Vec<Value>) {
    for part in &message.parts {
        match part {
            Part::Reasoning { opaque, .. } => {
                let Some(opaque) = opaque.as_ref().filter(|o| o.origin.can_replay_to(origin)) else {
                    continue;
                };
                let mut item = json!({"type": "reasoning", "summary": opaque.data["summary"].clone()});
                if !item["summary"].is_array() {
                    item["summary"] = json!([]);
                }
                if let Some(secret) = opaque.data.get("encrypted_content").filter(|v| v.is_string()) {
                    item["encrypted_content"] = secret.clone();
                }
                input.push(item);
            }
            Part::Text { text } => input.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            })),
            Part::ToolCall {
                call_id,
                name,
                input: args,
                ..
            } => input.push(json!({
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": args.to_string(),
            })),
            // 联网搜索的那一项：同一来源原样放回（不带 id，和上面一样）。
            Part::ServerTool { opaque } => {
                if opaque.origin.can_replay_to(origin) {
                    input.push(opaque.data.clone());
                }
            }
            Part::ToolResult { .. } | Part::Image { .. } => {}
        }
    }
}

/// 一个 `web_search_call` 项：搜的什么、看了哪些网页。
fn search_of(item: &Value) -> (Option<String>, Vec<Source>) {
    let action = &item["action"];
    let query = action["query"].as_str().map(str::to_string).or_else(|| {
        let qs: Vec<&str> = action["queries"].as_array().into_iter().flatten().filter_map(|q| q.as_str()).collect();
        (!qs.is_empty()).then(|| qs.join(" / "))
    });
    let query = query.or_else(|| action["url"].as_str().map(|u| format!("打开 {u}")));
    let sources = action["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let url = s["url"].as_str()?.to_string();
            Some(Source { title: s["title"].as_str().unwrap_or(&url).to_string(), url })
        })
        .collect();
    (query, sources)
}

pub struct ResponsesStream {
    origin: Origin,
    catalog_id: Option<String>,
    /// 百炼按地域不同价（新加坡）。
    region: Option<String>,
    /// `output_item.done` 收到的完整项（按位置）；`response.completed` 里有整份时以那份为准。
    items: BTreeMap<u64, Value>,
    output: Option<Vec<Value>>,
    usage: TokenUsage,
    status: Option<String>,
    incomplete: Option<String>,
    response_model: Option<String>,
    error: Option<String>,
}

impl ResponsesStream {
    pub fn new(origin: Origin, catalog_id: Option<String>, region: Option<String>) -> Self {
        Self {
            origin,
            catalog_id,
            region,
            items: BTreeMap::new(),
            output: None,
            usage: TokenUsage::default(),
            status: None,
            incomplete: None,
            response_model: None,
            error: None,
        }
    }

    fn take_response(&mut self, r: &Value) {
        if let Some(m) = r["model"].as_str() {
            self.response_model = Some(m.into());
        }
        if let Some(output) = r["output"].as_array().filter(|o| !o.is_empty()) {
            self.output = Some(output.clone());
        }
        let u = &r["usage"];
        if u.is_object() {
            // input_tokens 里含缓存读和缓存写，这里拆开（缓存写 2026 年起单独计费）。
            let details = &u["input_tokens_details"];
            let cached = details["cached_tokens"].as_u64();
            // 百炼的会话缓存写入叫 cache_creation_input_tokens。
            let written = details["cache_write_tokens"].as_u64().or_else(|| details["cache_creation_input_tokens"].as_u64());
            self.usage.input = minus(minus(u["input_tokens"].as_u64(), cached), written);
            self.usage.cache_read = cached;
            self.usage.cache_write = written;
            self.usage.output = u["output_tokens"].as_u64();
            self.usage.reasoning = u["output_tokens_details"]["reasoning_tokens"].as_u64();
        }
    }
}

impl WireStream for ResponsesStream {
    fn feed(&mut self, data: &Value) -> Vec<ModelEvent> {
        match data["type"].as_str().unwrap_or("") {
            "response.created" | "response.in_progress" => {
                self.take_response(&data["response"]);
                vec![]
            }
            "response.output_item.added" => {
                let item = &data["item"];
                if item["type"] == "function_call" {
                    return vec![ModelEvent::ToolCallStarted {
                        call_id: item["call_id"].as_str().unwrap_or("").into(),
                        name: item["name"].as_str().unwrap_or("").into(),
                    }];
                }
                if item["type"] == "web_search_call" {
                    return vec![ModelEvent::ServerToolStarted {
                        call_id: item["id"].as_str().unwrap_or("").into(),
                        name: "web_search".into(),
                        query: search_of(item).0,
                    }];
                }
                vec![]
            }
            "response.output_text.delta" => text(data, |t| ModelEvent::TextDelta { text: t }),
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                text(data, |t| ModelEvent::ReasoningDelta { text: t })
            }
            "response.reasoning_summary_part.added" => {
                // 摘要分段：第二段起前面空一行，和整条回答里拼起来的一样。
                if data["summary_index"].as_u64().unwrap_or(0) > 0 {
                    return vec![ModelEvent::ReasoningDelta { text: "\n\n".into() }];
                }
                vec![]
            }
            "response.output_item.done" => {
                let index = data["output_index"].as_u64().unwrap_or(self.items.len() as u64);
                let item = &data["item"];
                self.items.insert(index, item.clone());
                if item["type"] == "web_search_call" {
                    let (query, sources) = search_of(item);
                    let error = (item["status"] == "failed").then(|| "没搜成".to_string());
                    return vec![ModelEvent::ServerToolDone { call_id: item["id"].as_str().unwrap_or("").into(), query, sources, error }];
                }
                vec![]
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                let r = &data["response"];
                self.take_response(r);
                self.status = r["status"].as_str().map(str::to_string).or_else(|| {
                    data["type"].as_str().map(|t| t.trim_start_matches("response.").to_string())
                });
                self.incomplete = r["incomplete_details"]["reason"].as_str().map(str::to_string);
                if data["type"] == "response.failed" {
                    self.error = Some(error_text(&r["error"]));
                }
                vec![]
            }
            "error" => {
                self.error = Some(error_text(data));
                vec![]
            }
            _ => vec![],
        }
    }

    fn done(&self) -> bool {
        self.status.is_some()
    }

    fn finish(self: Box<Self>) -> Result<ModelResponse, String> {
        let this = *self;
        if let Some(e) = this.error {
            return Err(e);
        }
        let Some(status) = this.status else {
            return Err("回答在结束前断开了，没自动重发。".into());
        };
        let output = this
            .output
            .unwrap_or_else(|| this.items.into_values().collect());
        let mut parts = Vec::new();
        let mut refused = false;
        let mut usage = this.usage;
        for item in &output {
            match item["type"].as_str().unwrap_or("") {
                "reasoning" => {
                    let summary = item["summary"].as_array().cloned().unwrap_or_default();
                    let text = summary
                        .iter()
                        .filter_map(|s| s["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let mut data = json!({"type": "reasoning", "summary": summary});
                    if let Some(secret) = item.get("encrypted_content").filter(|v| v.is_string()) {
                        data["encrypted_content"] = secret.clone();
                    }
                    parts.push(Part::Reasoning {
                        text,
                        opaque: Some(OpaqueReasoning {
                            origin: this.origin.clone(),
                            data,
                        }),
                    });
                }
                "message" => {
                    for c in item["content"].as_array().into_iter().flatten() {
                        match c["type"].as_str().unwrap_or("") {
                            "output_text" => parts.push(Part::Text {
                                text: c["text"].as_str().unwrap_or("").into(),
                            }),
                            "refusal" => {
                                refused = true;
                                parts.push(Part::Text {
                                    text: c["refusal"].as_str().unwrap_or("").into(),
                                })
                            }
                            _ => {}
                        }
                    }
                }
                "function_call" => {
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    let raw = item["arguments"].as_str().unwrap_or("");
                    let input = if raw.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(raw).map_err(|_| format!("工具 {name} 的参数不完整（可能被截断）"))?
                    };
                    parts.push(Part::ToolCall {
                        call_id: item["call_id"].as_str().unwrap_or("").into(),
                        name,
                        input,
                        signature: None,
                    });
                }
                "web_search_call" => {
                    // 一次「搜」收一次钱；打开网页、页内查找不算。
                    if item["action"]["type"] == "search" {
                        usage.web_searches = Some(usage.web_searches.unwrap_or(0) + 1);
                    }
                    let mut data = item.clone();
                    if let Some(o) = data.as_object_mut() {
                        o.remove("id");
                    }
                    parts.push(Part::ServerTool { opaque: OpaqueReasoning { origin: this.origin.clone(), data } });
                }
                _ => {}
            }
        }
        let stop = if refused {
            StopReason::Refusal
        } else if status == "incomplete" {
            match this.incomplete.as_deref() {
                Some("max_output_tokens") => StopReason::MaxTokens,
                Some("content_filter") => StopReason::Refusal,
                Some(other) => StopReason::Other(other.into()),
                None => StopReason::Other("incomplete".into()),
            }
        } else if parts.iter().any(|p| matches!(p, Part::ToolCall { .. })) {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };
        let cost = priced(this.catalog_id.as_deref(), this.region.as_deref(), &usage);
        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                parts,
            },
            stop,
            usage,
            cost,
            response_model: this.response_model,
        })
    }
}

fn text(data: &Value, make: impl Fn(String) -> ModelEvent) -> Vec<ModelEvent> {
    match data["delta"].as_str() {
        Some(t) if !t.is_empty() => vec![make(t.into())],
        _ => vec![],
    }
}

fn error_text(e: &Value) -> String {
    let message = e["message"]
        .as_str()
        .or_else(|| e["error"]["message"].as_str());
    match message {
        Some(m) => format!("供应商在回答中途报错：{}", super::short(m, 160)),
        None => "供应商在回答中途报错。".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::catalog::catalog;
    use crate::gateway::event::Wire;
    use crate::gateway::{SystemBlock, ToolSpec};

    const MODEL: &str = "openai/gpt-6-luna";

    fn origin() -> Origin {
        Origin {
            provider: "openai".into(),
            wire: Wire::Responses,
            model: "gpt-6-luna".into(),
        }
    }

    fn run(lines: &[Value]) -> (Vec<ModelEvent>, Result<ModelResponse, String>) {
        let mut s: Box<dyn WireStream> = Box::new(ResponsesStream::new(origin(), Some(MODEL.into()), None));
        let events = lines.iter().flat_map(|l| s.feed(l)).collect();
        (events, s.finish())
    }

    #[test]
    fn reasoning_secret_and_tool_call_round_trip_without_ids() {
        let reasoning = json!({"id": "rs_1", "type": "reasoning", "summary": [{"type": "summary_text", "text": "先读文件"}], "encrypted_content": "gAAA"});
        let call = json!({"id": "fc_1", "type": "function_call", "status": "completed", "call_id": "call_9", "name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"});
        let (events, response) = run(&[
            json!({"type": "response.created", "response": {"model": "gpt-6-luna", "output": []}}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "先读文件"}),
            json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "function_call", "call_id": "call_9", "name": "read_file"}}),
            json!({"type": "response.completed", "response": {"status": "completed", "model": "gpt-6-luna-2026", "output": [reasoning, call],
                "usage": {"input_tokens": 120, "input_tokens_details": {"cached_tokens": 100}, "output_tokens": 40, "output_tokens_details": {"reasoning_tokens": 30}}}}),
        ]);
        let response = response.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(response.stop, StopReason::ToolUse);
        assert_eq!(response.usage.input, Some(20));
        assert_eq!(response.usage.cache_read, Some(100));
        assert_eq!(response.response_model.as_deref(), Some("gpt-6-luna-2026"));

        let model = catalog().model(MODEL).unwrap();
        let mut messages = vec![Message {
            role: Role::User,
            parts: vec![Part::Text { text: "几条？".into() }],
        }];
        messages.push(response.message.clone());
        messages.push(Message {
            role: Role::User,
            parts: vec![Part::ToolResult {
                call_id: "call_9".into(),
                output: "1\t- a".into(),
                is_error: false,
            }],
        });
        let request = ModelRequest {
            model: MODEL.into(),
            system: vec![SystemBlock { text: "简短回答".into(), cache: false }],
            messages,
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: "读文件".into(),
                input_schema: json!({"type": "object"}),
            }],
            thinking: true,
            effort: Some("low".into()),
            max_output_tokens: 1000,
            web_search: false,
        };
        let body = encode(&request, model, &origin(), Dialect::OpenAi);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "简短回答");
        assert_eq!(body["reasoning"], json!({"summary": "auto", "effort": "low"}));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[1], json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "先读文件"}], "encrypted_content": "gAAA"}));
        assert_eq!(input[2], json!({"type": "function_call", "call_id": "call_9", "name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"}));
        assert_eq!(input[3]["type"], "function_call_output");

        // 换一家：思考丢掉，调用和结果照旧。
        let other = Origin { model: "gpt-6.1-sol".into(), ..origin() };
        let body = encode(&request, model, &other, Dialect::OpenAi);
        assert!(body["input"].as_array().unwrap().iter().all(|i| i["type"] != "reasoning"));
    }

    #[test]
    fn cut_streams_and_truncated_answers_are_not_success() {
        let (_, cut) = run(&[json!({"type": "response.output_text.delta", "delta": "半句"})]);
        assert!(cut.is_err());
        let (_, short) = run(&[json!({"type": "response.incomplete", "response": {"status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"},
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "半句"}]}]}})]);
        assert_eq!(short.unwrap().stop, StopReason::MaxTokens);
        let (_, failed) = run(&[json!({"type": "response.failed", "response": {"status": "failed", "error": {"message": "overloaded"}}})]);
        assert!(failed.unwrap_err().contains("overloaded"));
    }

    #[test]
    fn web_search_calls_are_shown_counted_and_replayed_without_ids() {
        let model = catalog().model(MODEL).unwrap();
        let user = Message { role: Role::User, parts: vec![Part::Text { text: "Tauri 最新版？".into() }] };
        let mut request = ModelRequest {
            model: MODEL.into(),
            system: vec![],
            messages: vec![user.clone()],
            tools: vec![],
            thinking: false,
            effort: None,
            max_output_tokens: 1000,
            web_search: true,
        };
        let body = encode(&request, model, &origin(), Dialect::OpenAi);
        assert_eq!(body["tools"], json!([{"type": "web_search"}]));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content", "web_search_call.action.sources"]));

        let call = json!({"id": "ws_1", "type": "web_search_call", "status": "completed", "action": {"type": "search", "query": "tauri 2 release", "sources": [{"type": "url", "url": "https://tauri.app/release"}]}});
        let open = json!({"id": "ws_2", "type": "web_search_call", "status": "completed", "action": {"type": "open_page", "url": "https://tauri.app/release"}});
        let message = json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "最新是 2.9。", "annotations": [{"type": "url_citation", "url": "https://tauri.app/release"}]}]});
        let (events, response) = run(&[
            json!({"type": "response.output_item.added", "output_index": 0, "item": {"id": "ws_1", "type": "web_search_call", "status": "in_progress"}}),
            json!({"type": "response.output_item.done", "output_index": 0, "item": call}),
            json!({"type": "response.output_item.done", "output_index": 1, "item": open}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [call, open, message], "usage": {"input_tokens": 500, "output_tokens": 40}}}),
        ]);
        assert_eq!(events[0], ModelEvent::ServerToolStarted { call_id: "ws_1".into(), name: "web_search".into(), query: None });
        assert_eq!(
            events[1],
            ModelEvent::ServerToolDone { call_id: "ws_1".into(), query: Some("tauri 2 release".into()), sources: vec![Source { title: "https://tauri.app/release".into(), url: "https://tauri.app/release".into() }], error: None }
        );
        let response = response.unwrap();
        assert_eq!(response.usage.web_searches, Some(1), "打开网页不算一次搜索");
        assert_eq!(response.stop, StopReason::EndTurn);

        request.messages.push(response.message.clone());
        let body = encode(&request, model, &origin(), Dialect::OpenAi);
        let replayed: Vec<&Value> = body["input"].as_array().unwrap().iter().filter(|i| i["type"] == "web_search_call").collect();
        assert_eq!(replayed.len(), 2);
        assert!(replayed.iter().all(|i| i.get("id").is_none() && i["action"].is_object()), "不存对话：回放不带 id");
        let other = Origin { model: "gpt-6-astra".into(), ..origin() };
        let body = encode(&request, model, &other, Dialect::OpenAi);
        assert!(!body["input"].as_array().unwrap().iter().any(|i| i["type"] == "web_search_call"), "换了模型不带");
    }

    #[test]
    fn qwen_on_responses_uses_enable_thinking_its_own_search_and_regional_prices() {
        let qwen = catalog().model("aliyun/qwen3.8-flash").unwrap();
        let origin = Origin { provider: "aliyun".into(), wire: Wire::Responses, model: "qwen3.8-flash".into() };
        let user = Message { role: Role::User, parts: vec![Part::Text { text: "Tauri 最新版？".into() }] };
        let mut request = ModelRequest {
            model: "aliyun/qwen3.8-flash".into(),
            system: vec![],
            messages: vec![user],
            tools: vec![ToolSpec { name: "read_file".into(), description: "读".into(), input_schema: json!({"type": "object"}) }],
            thinking: true,
            effort: Some("low".into()),
            max_output_tokens: 1000,
            web_search: true,
        };
        let body = encode(&request, qwen, &origin, Dialect::Qwen);
        assert_eq!((body["enable_thinking"].as_bool(), body["reasoning"].clone()), (Some(true), json!({"effort": "low"})));
        assert!(body.get("include").is_none(), "百炼不发 include");
        assert_eq!(body["tools"][1], json!({"type": "web_search"}), "函数工具和 web_search 放在一起");
        request.thinking = false;
        let body = encode(&request, qwen, &origin, Dialect::Qwen);
        assert_eq!(body["enable_thinking"], false);
        assert!(body.get("reasoning").is_none(), "关思考不发 effort none");

        // 流：思考在 reasoning_text.delta；新加坡按地域价。
        let mut s: Box<dyn WireStream> = Box::new(ResponsesStream::new(origin, Some("aliyun/qwen3.8-flash".into()), Some("singapore".into())));
        let mut events = s.feed(&json!({"type": "response.reasoning_text.delta", "delta": "想一下"}));
        events.extend(s.feed(&json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "reasoning", "id": "msg_1", "summary": [{"type": "summary_text", "text": "想一下"}]},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "好"}]}
        ], "usage": {"input_tokens": 1000, "input_tokens_details": {"cached_tokens": 600}, "output_tokens": 10}}})));
        assert_eq!(events, vec![ModelEvent::ReasoningDelta { text: "想一下".into() }]);
        let r = s.finish().unwrap();
        assert_eq!((r.usage.input, r.usage.cache_read), (Some(400), Some(600)));
        let price = &qwen.region_prices["singapore"];
        let want = (400.0 * price.input + 10.0 * price.output + 600.0 * price.cache_read.unwrap_or(price.input)) / 1e6;
        assert!((r.cost.usd.unwrap() - want).abs() < 1e-12, "{:?} vs {want}", r.cost.usd);
    }
}
