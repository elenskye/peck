//! Anthropic Messages（流式 SSE）→ 裸模型的增量和整条回答。Anthropic 和 Vertex 上的 Claude 共用。
//!
//! 思考块（`thinking` 带 `signature`、`redacted_thinking`）原样挂进 `OpaqueReasoning`，
//! 回放时 [`assistant_blocks`] 只在来源一样时放回去，换了就丢。
//!
//! 联网搜索（v1.1.1）：请求带 `web_search` 服务端工具；回答里的 `server_tool_use` 和 `*_tool_result` 块原样存成
//! `Part::ServerTool`（同样只回放给同一来源），搜了几次记进用量；搜到一半先停的 `pause_turn` 是 `StopReason::Paused`。

use crate::gateway::catalog::{catalog, ModelSpec, Thinking};
use crate::gateway::event::{OpaqueReasoning, Origin, TokenUsage, Wire};
use crate::gateway::search::{self, Native, Source};
use crate::gateway::{Message, ModelEvent, ModelRequest, ModelResponse, Part, Role, StopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一段 SSE 原文 → 每条 `data:` 的 JSON（`ping` 也在里面，转换时跳过）。
#[cfg(test)]
pub fn parse_sse(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .filter_map(|d| serde_json::from_str(d.trim()).ok())
        .collect()
}

enum Block {
    Thinking { text: String, signature: String },
    Redacted { data: Value },
    Text(String),
    ToolUse { id: String, name: String, json: String },
    /// 供应商那边跑的工具（`server_tool_use`）：原件 + 流进来的参数。
    ServerUse { data: Value, json: String },
    /// 它的结果（`web_search_tool_result` 这类）：原件。
    ServerResult { data: Value },
    Other,
}

/// 一次请求的流：一条一条喂 `data:` 的 JSON，`finish` 拿整条回答。
pub struct AnthropicStream {
    origin: Origin,
    /// 目录里的模型 id（算价格用）。
    catalog_id: Option<String>,
    blocks: BTreeMap<u64, Block>,
    usage: TokenUsage,
    stop: Option<StopReason>,
    response_model: Option<String>,
    error: Option<String>,
    stopped: bool,
}

impl AnthropicStream {
    pub fn new(origin: Origin, catalog_id: Option<String>) -> Self {
        Self {
            origin,
            catalog_id,
            blocks: BTreeMap::new(),
            usage: TokenUsage::default(),
            stop: None,
            response_model: None,
            error: None,
            stopped: false,
        }
    }

    fn take_usage(&mut self, u: &Value) {
        let n = |k: &str| u[k].as_u64();
        if let Some(v) = n("input_tokens") {
            self.usage.input = Some(v);
        }
        if let Some(v) = n("cache_read_input_tokens") {
            self.usage.cache_read = Some(v);
        }
        if let Some(v) = n("cache_creation_input_tokens") {
            self.usage.cache_write = Some(v);
        }
        if let Some(v) = n("output_tokens") {
            self.usage.output = Some(v);
        }
        if let Some(v) = u["output_tokens_details"]["thinking_tokens"].as_u64() {
            self.usage.reasoning = Some(v);
        }
        if let Some(v) = u["server_tool_use"]["web_search_requests"].as_u64() {
            self.usage.web_searches = Some(v);
        }
    }

    /// 一个服务端工具调用搜的是什么（参数流完了才有）。
    fn query_of(&self, call_id: &str) -> Option<String> {
        self.blocks.values().find_map(|b| match b {
            Block::ServerUse { data, json } if data["id"] == call_id => {
                let input: Value = serde_json::from_str(json).ok().unwrap_or_else(|| data["input"].clone());
                input["query"].as_str().map(str::to_string)
            }
            _ => None,
        })
    }

    pub fn feed(&mut self, data: &Value) -> Vec<ModelEvent> {
        match data["type"].as_str().unwrap_or("") {
            "message_start" => {
                let m = &data["message"];
                self.response_model = m["model"].as_str().map(str::to_string);
                self.take_usage(&m["usage"]);
                vec![]
            }
            "content_block_start" => {
                let index = data["index"].as_u64().unwrap_or(0);
                let b = &data["content_block"];
                let (block, event) = match b["type"].as_str().unwrap_or("") {
                    "thinking" => (
                        Block::Thinking {
                            text: b["thinking"].as_str().unwrap_or("").into(),
                            signature: b["signature"].as_str().unwrap_or("").into(),
                        },
                        None,
                    ),
                    "redacted_thinking" => (Block::Redacted { data: b.clone() }, None),
                    "text" => (Block::Text(b["text"].as_str().unwrap_or("").into()), None),
                    "tool_use" => {
                        let id = b["id"].as_str().unwrap_or("").to_string();
                        let name = b["name"].as_str().unwrap_or("").to_string();
                        (
                            Block::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                json: String::new(),
                            },
                            Some(ModelEvent::ToolCallStarted { call_id: id, name }),
                        )
                    }
                    "server_tool_use" => {
                        let started = ModelEvent::ServerToolStarted {
                            call_id: b["id"].as_str().unwrap_or("").into(),
                            name: b["name"].as_str().unwrap_or("").into(),
                            query: b["input"]["query"].as_str().map(str::to_string),
                        };
                        (Block::ServerUse { data: b.clone(), json: String::new() }, Some(started))
                    }
                    t if t.ends_with("_tool_result") && b["tool_use_id"].is_string() => {
                        let call_id = b["tool_use_id"].as_str().unwrap_or("").to_string();
                        let (sources, error) = result_of(&b["content"]);
                        let done = ModelEvent::ServerToolDone { query: self.query_of(&call_id), call_id, sources, error };
                        (Block::ServerResult { data: b.clone() }, Some(done))
                    }
                    _ => (Block::Other, None),
                };
                self.blocks.insert(index, block);
                event.into_iter().collect()
            }
            "content_block_delta" => {
                let index = data["index"].as_u64().unwrap_or(0);
                let d = &data["delta"];
                let Some(block) = self.blocks.get_mut(&index) else {
                    return vec![];
                };
                match (block, d["type"].as_str().unwrap_or("")) {
                    (Block::Thinking { text, .. }, "thinking_delta") => {
                        let t = d["thinking"].as_str().unwrap_or("");
                        text.push_str(t);
                        vec![ModelEvent::ReasoningDelta { text: t.into() }]
                    }
                    (Block::Thinking { signature, .. }, "signature_delta") => {
                        signature.push_str(d["signature"].as_str().unwrap_or(""));
                        vec![]
                    }
                    (Block::Text(text), "text_delta") => {
                        let t = d["text"].as_str().unwrap_or("");
                        text.push_str(t);
                        vec![ModelEvent::TextDelta { text: t.into() }]
                    }
                    (Block::ToolUse { json, .. } | Block::ServerUse { json, .. }, "input_json_delta") => {
                        json.push_str(d["partial_json"].as_str().unwrap_or(""));
                        vec![]
                    }
                    _ => vec![],
                }
            }
            "message_delta" => {
                self.take_usage(&data["usage"]);
                self.stop = data["delta"]["stop_reason"].as_str().map(stop_reason);
                vec![]
            }
            "message_stop" => {
                self.stopped = true;
                vec![]
            }
            "error" => {
                self.error = Some(
                    data["error"]["message"]
                        .as_str()
                        .map(|m| format!("供应商在回答中途报错：{}", crate::gateway::convert::short(m, 160)))
                        .unwrap_or_else(|| "供应商在回答中途报错。".into()),
                );
                vec![]
            }
            _ => vec![],
        }
    }

    /// 流结束：整条回答。流里报过错或工具参数不是完整 JSON（被 `max_tokens` 截断）返回 `Err`。
    pub fn finish(self) -> Result<ModelResponse, String> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let mut parts = Vec::new();
        for (_, block) in self.blocks {
            match block {
                Block::Thinking { text, signature } => parts.push(Part::Reasoning {
                    text,
                    opaque: Some(OpaqueReasoning {
                        origin: self.origin.clone(),
                        data: json!({"type": "thinking", "signature": signature}),
                    }),
                }),
                Block::Redacted { data } => parts.push(Part::Reasoning {
                    text: String::new(),
                    opaque: Some(OpaqueReasoning {
                        origin: self.origin.clone(),
                        data,
                    }),
                }),
                Block::Text(text) => parts.push(Part::Text { text }),
                Block::ToolUse { id, name, json } => {
                    let input = if json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&json).map_err(|_| format!("工具 {name} 的参数不完整（可能被截断）"))?
                    };
                    parts.push(Part::ToolCall {
                        call_id: id,
                        name,
                        input,
                        signature: None,
                    })
                }
                Block::ServerUse { mut data, json } => {
                    if let Ok(input) = serde_json::from_str::<Value>(&json) {
                        data["input"] = input;
                    }
                    parts.push(Part::ServerTool { opaque: OpaqueReasoning { origin: self.origin.clone(), data } })
                }
                Block::ServerResult { data } => parts.push(Part::ServerTool { opaque: OpaqueReasoning { origin: self.origin.clone(), data } }),
                Block::Other => {}
            }
        }
        let cost = self
            .catalog_id
            .as_deref()
            .and_then(|id| catalog().model(id))
            .map(|m| m.cost(&self.usage, None))
            .unwrap_or_else(crate::gateway::event::Cost::unknown);
        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                parts,
            },
            stop: self.stop.unwrap_or(StopReason::Other("missing".into())),
            usage: self.usage,
            cost,
            response_model: self.response_model,
        })
    }
}

fn stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "pause_turn" => StopReason::Paused,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" | "model_context_window_exceeded" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal,
        other => StopReason::Other(other.into()),
    }
}

/// 一条助手消息 → Anthropic 的 content 块（下一轮原样放回）。思考只在来源和 `target` 一样时放回。
pub fn assistant_blocks(message: &Message, target: &Origin) -> Vec<Value> {
    let mut out = Vec::new();
    for part in &message.parts {
        match part {
            Part::Reasoning { text, opaque } => {
                let Some(opaque) = opaque.as_ref().filter(|o| o.origin.can_replay_to(target)) else {
                    continue;
                };
                if opaque.data["type"] == "redacted_thinking" {
                    out.push(opaque.data.clone());
                } else {
                    out.push(json!({"type": "thinking", "thinking": text, "signature": opaque.data["signature"]}));
                }
            }
            Part::Text { text } => out.push(json!({"type": "text", "text": text})),
            Part::ToolCall {
                call_id, name, input, ..
            } => out.push(json!({"type": "tool_use", "id": call_id, "name": name, "input": input})),
            Part::ToolResult {
                call_id,
                output,
                is_error,
            } => {
                let mut b = json!({"type": "tool_result", "tool_use_id": call_id, "content": output});
                if *is_error {
                    b["is_error"] = json!(true);
                }
                out.push(b)
            }
            Part::Image { media_type, data } => {
                out.push(json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}))
            }
            // 联网搜索的原件：同一来源原样放回（加密的结果、搜索的调用都要带着），换了就丢。
            Part::ServerTool { opaque } => {
                if opaque.origin.can_replay_to(target) {
                    out.push(opaque.data.clone());
                }
            }
        }
    }
    out
}

/// 一个服务端工具结果的内容 → 找到的网页，或者出错的原因（出错时 `content` 是一个带 `error_code` 的对象）。
fn result_of(content: &Value) -> (Vec<Source>, Option<String>) {
    if let Some(code) = content["error_code"].as_str() {
        let why = match code {
            "max_uses_exceeded" => "这一轮搜的次数到上限了".to_string(),
            "too_many_requests" => "搜得太频繁了".to_string(),
            "query_too_long" => "搜的话太长了".to_string(),
            "unavailable" => "搜索暂时用不了".to_string(),
            other => format!("没搜成（{other}）"),
        };
        return (vec![], Some(why));
    }
    let sources = content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let url = r["url"].as_str()?.to_string();
            Some(Source { title: r["title"].as_str().unwrap_or(&url).to_string(), url })
        })
        .collect();
    (sources, None)
}

impl super::WireStream for AnthropicStream {
    fn feed(&mut self, data: &Value) -> Vec<ModelEvent> {
        AnthropicStream::feed(self, data)
    }

    fn done(&self) -> bool {
        self.stopped
    }

    fn finish(self: Box<Self>) -> Result<ModelResponse, String> {
        if self.error.is_none() && self.stop.is_none() {
            return Err("回答在结束前断开了，没自动重发。".into());
        }
        AnthropicStream::finish(*self)
    }
}

/// Vertex 上的 Claude 的 body 里要带的版本（模型名在 URL 里，body 不放 `model`）。
pub const VERTEX_VERSION: &str = "vertex-2023-10-16";

/// 统一的请求 → Messages 的 body。`vertex` 为真：不放 `model`、加 `anthropic_version`、不开
/// `eager_input_streaming`（Vertex 不一定认）。思考只回放来源和 `origin` 一样的。
pub fn encode(request: &ModelRequest, model: &ModelSpec, origin: &Origin, vertex: bool) -> Value {
    let mut body = json!({"max_tokens": request.max_output_tokens, "stream": true});
    if vertex {
        body["anthropic_version"] = json!(VERTEX_VERSION);
    } else {
        body["model"] = json!(model.api_model);
    }
    if !request.system.is_empty() {
        let blocks: Vec<Value> = request
            .system
            .iter()
            .map(|b| {
                let mut v = json!({"type": "text", "text": b.text});
                if b.cache {
                    v["cache_control"] = json!({"type": "ephemeral"});
                }
                v
            })
            .collect();
        body["system"] = Value::Array(blocks);
    }
    let messages: Vec<Value> = request
        .messages
        .iter()
        .filter_map(|m| {
            let (role, content) = match m.role {
                Role::Assistant => ("assistant", assistant_blocks(m, origin)),
                Role::User => ("user", user_blocks(m)),
            };
            (!content.is_empty()).then(|| json!({"role": role, "content": content}))
        })
        .collect();
    // 对话的缓存断点（v0.6）：系统提示打了断点时，最后一条站主消息的最后一块也打一个，下一次请求接着它命中缓存。
    // 系统提示两块 + 这一个，不超过 Messages 的 4 个。
    let mut messages = messages;
    if request.system.iter().any(|b| b.cache) {
        if let Some(last) = messages.last_mut().filter(|m| m["role"] == "user") {
            if let Some(block) = last["content"].as_array_mut().and_then(|c| c.last_mut()) {
                block["cache_control"] = json!({"type": "ephemeral"});
            }
        }
    }
    body["messages"] = Value::Array(messages);
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let mut v = json!({"name": t.name, "description": t.description, "input_schema": t.input_schema});
                if !vertex {
                    v["eager_input_streaming"] = json!(true);
                }
                v
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    // 联网搜索（v1.1.1）：Anthropic 自己的服务端工具，放在 Peck 的工具后面。
    if let (true, Some(Native::Anthropic(kind))) = (request.web_search, search::native(model)) {
        let tool = json!({"type": kind, "name": "web_search", "max_uses": search::MAX_USES});
        match body["tools"].as_array_mut() {
            Some(tools) => tools.push(tool),
            None => body["tools"] = json!([tool]),
        }
    }
    match (model.thinking, request.thinking) {
        (Thinking::Always, _) | (Thinking::Switch, true) => {
            body["thinking"] = json!({"type": "adaptive", "display": "summarized"})
        }
        (Thinking::Switch, false) => body["thinking"] = json!({"type": "disabled"}),
        (Thinking::Never, _) => {}
    }
    if let Some(effort) = &request.effort {
        body["output_config"] = json!({"effort": effort});
    }
    body
}

/// 站主这一条 → content 块：工具结果放前面（Messages 要求），再放文字。
fn user_blocks(message: &Message) -> Vec<Value> {
    let mut results = Vec::new();
    let mut texts = Vec::new();
    for part in &message.parts {
        match part {
            Part::ToolResult { .. } => results.extend(assistant_blocks(
                &Message {
                    role: Role::User,
                    parts: vec![part.clone()],
                },
                &Origin {
                    provider: String::new(),
                    wire: Wire::Messages,
                    model: String::new(),
                },
            )),
            Part::Text { text } => texts.push(json!({"type": "text", "text": text})),
            Part::Image { media_type, data } => {
                texts.push(json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}))
            }
            Part::Reasoning { .. } | Part::ToolCall { .. } | Part::ServerTool { .. } => {}
        }
    }
    results.extend(texts);
    results
}

/// 目录里的模型 → 回放来源（供应商 + Messages + 发给供应商的名字）。
#[cfg(test)]
pub fn origin_for(catalog_id: &str) -> Option<Origin> {
    let m = catalog().model(catalog_id)?;
    Some(Origin {
        provider: m.provider.clone(),
        wire: Wire::Messages,
        model: m.api_model.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::convert::fixtures;
    use crate::gateway::event::CostSource;

    const NAME: &str = "anthropic-tools-thinking";
    const MODEL: &str = "anthropic/claude-haiku-5-5";

    fn run(file: &str) -> (Vec<ModelEvent>, ModelResponse) {
        let mut stream = AnthropicStream::new(origin_for(MODEL).unwrap(), Some(MODEL.into()));
        let mut events = Vec::new();
        for data in parse_sse(&fixtures::read(NAME, file)) {
            events.extend(stream.feed(&data));
        }
        (events, stream.finish().unwrap())
    }

    fn check_json(file: &str, actual: Value) {
        let path = fixtures::dir(NAME).join(file);
        if std::env::var_os("PECK_REGEN_FIXTURES").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
            return;
        }
        let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(expected, actual, "{file} 不一样");
    }

    #[test]
    fn images_go_after_tool_results_as_base64_blocks() {
        let m = Message {
            role: Role::User,
            parts: vec![
                Part::Image { media_type: "image/png".into(), data: "AAAA".into() },
                Part::ToolResult { call_id: "c1".into(), output: "截好了".into(), is_error: false },
            ],
        };
        let blocks = user_blocks(&m);
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        let gemini = crate::gateway::convert::gemini::user_parts_for_test(&m);
        assert!(gemini.iter().any(|p| p["inlineData"]["mimeType"] == "image/png"));
    }

    #[test]
    fn tool_call_request_becomes_a_tool_call_with_priced_usage() {
        let (events, response) = run("req1.sse");
        check_json("req1.expected.json", json!({"events": events, "response": response}));
        assert_eq!(response.stop, StopReason::ToolUse);
        assert!(matches!(&response.message.parts[..], [Part::ToolCall { name, .. }] if name == "read_file"));
        assert!(matches!(events.first(), Some(ModelEvent::ToolCallStarted { .. })));
        assert_eq!(response.cost.source, CostSource::Priced);
    }

    #[test]
    fn thinking_keeps_its_signature_and_counts_thinking_tokens() {
        let (events, response) = run("req2.sse");
        check_json("req2.expected.json", json!({"events": events, "response": response}));
        let Part::Reasoning { text, opaque } = &response.message.parts[0] else {
            panic!("第一块应该是思考");
        };
        assert!(text.contains("67"));
        let opaque = opaque.as_ref().unwrap();
        assert!(opaque.data["signature"].as_str().unwrap().len() > 100);
        assert_eq!(response.usage.reasoning, Some(108));
        assert!(response.usage.output.unwrap() >= response.usage.reasoning.unwrap());
        // 0.10 / 0.50 每百万：599 进、143 出
        let usd = response.cost.usd.unwrap();
        assert!((usd - (599.0 * 0.1 + 143.0 * 0.5) / 1e6).abs() < 1e-12);
    }

    #[test]
    fn cached_system_also_caches_the_tail_of_the_conversation() {
        use crate::gateway::{Message, ModelRequest, Role, SystemBlock};
        let model = catalog().model(MODEL).unwrap();
        let origin = origin_for(MODEL).unwrap();
        let mut request = ModelRequest {
            model: MODEL.into(),
            system: vec![SystemBlock { text: "a".into(), cache: true }, SystemBlock { text: "b".into(), cache: true }],
            messages: vec![
                Message { role: Role::User, parts: vec![Part::Text { text: "一".into() }] },
                Message { role: Role::Assistant, parts: vec![Part::Text { text: "二".into() }] },
                Message { role: Role::User, parts: vec![Part::Text { text: "环境".into() }, Part::Text { text: "三".into() }] },
            ],
            tools: vec![],
            thinking: false,
            effort: None,
            max_output_tokens: 1024,
            web_search: false,
        };
        let body = encode(&request, model, &origin, false);
        let marks = body.to_string().matches("cache_control").count();
        assert_eq!(marks, 3, "系统两块 + 对话最后一块");
        assert_eq!(body["messages"][2]["content"][1]["cache_control"], json!({"type": "ephemeral"}));
        assert!(body["messages"][2]["content"][0].get("cache_control").is_none());
        request.system[0].cache = false;
        request.system[1].cache = false;
        assert!(!encode(&request, model, &origin, false).to_string().contains("cache_control"));
    }

    #[test]
    fn replayed_turn_matches_what_was_sent_and_drops_foreign_thinking() {
        let (_, first) = run("req1.sse");
        let mut sent: Value = serde_json::from_str(&fixtures::read(NAME, "req2.request.json")).unwrap();
        // 录的时候把整块原样发回去了，里面有 API 自己加的 `caller: direct`（默认值，不发也一样）。
        for block in sent["messages"][1]["content"].as_array_mut().unwrap() {
            if block["caller"] == json!({"type": "direct"}) {
                block.as_object_mut().unwrap().remove("caller");
            }
        }
        let target = origin_for(MODEL).unwrap();
        assert_eq!(Value::Array(assistant_blocks(&first.message, &target)), sent["messages"][1]["content"]);

        let (_, second) = run("req2.sse");
        assert_eq!(assistant_blocks(&second.message, &target)[0]["type"], "thinking");
        let elsewhere = Origin {
            provider: "vertex-anthropic".into(),
            ..target
        };
        let blocks = assistant_blocks(&second.message, &elsewhere);
        assert!(blocks.iter().all(|b| b["type"] != "thinking"), "换了供应商不该带思考");
    }

    fn ask(model: &str, messages: Vec<Message>) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: vec![],
            messages,
            tools: vec![crate::gateway::ToolSpec { name: "read_file".into(), description: "读".into(), input_schema: json!({"type": "object"}) }],
            thinking: false,
            effort: None,
            max_output_tokens: 1000,
            web_search: true,
        }
    }

    #[test]
    fn web_search_is_a_server_tool_whose_blocks_replay_to_the_same_model_only() {
        // 请求：Opus 用带动态过滤的版本，Haiku 用基础版；放在 Peck 的工具后面。
        let user = Message { role: Role::User, parts: vec![Part::Text { text: "Tauri 最新版？".into() }] };
        let opus = catalog().model("anthropic/claude-opus-5-5").unwrap();
        let origin = origin_for("anthropic/claude-opus-5-5").unwrap();
        let body = encode(&ask("anthropic/claude-opus-5-5", vec![user.clone()]), opus, &origin, false);
        assert_eq!(body["tools"][1], json!({"type": "web_search_20260209", "name": "web_search", "max_uses": 8}));
        let haiku = catalog().model(MODEL).unwrap();
        let body = encode(&ask(MODEL, vec![user.clone()]), haiku, &origin_for(MODEL).unwrap(), false);
        assert_eq!(body["tools"][1]["type"], "web_search_20250305");
        let mut off = ask(MODEL, vec![user.clone()]);
        off.web_search = false;
        assert_eq!(encode(&off, haiku, &origin_for(MODEL).unwrap(), false)["tools"].as_array().unwrap().len(), 1);

        // 流：搜索的调用（参数流进来）、结果（带来源）、回答；用量里搜了 1 次。
        let mut stream = AnthropicStream::new(origin_for(MODEL).unwrap(), Some(MODEL.into()));
        let lines = [
            json!({"type": "message_start", "message": {"model": "claude-haiku-5-5", "usage": {"input_tokens": 900, "output_tokens": 1}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\": \"tauri 2"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": " release\"}"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": [
                {"type": "web_search_result", "url": "https://tauri.app/release", "title": "Tauri 2.9", "encrypted_content": "EqgfCio", "page_age": "2026-11-01"}
            ]}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "text", "text": "", "citations": []}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "text_delta", "text": "最新是 2.9。"}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 30, "server_tool_use": {"web_search_requests": 1}}}),
            json!({"type": "message_stop"}),
        ];
        let events: Vec<ModelEvent> = lines.iter().flat_map(|l| stream.feed(l)).collect();
        assert_eq!(events[0], ModelEvent::ServerToolStarted { call_id: "srvtoolu_1".into(), name: "web_search".into(), query: None });
        assert_eq!(
            events[1],
            ModelEvent::ServerToolDone {
                call_id: "srvtoolu_1".into(),
                query: Some("tauri 2 release".into()),
                sources: vec![Source { title: "Tauri 2.9".into(), url: "https://tauri.app/release".into() }],
                error: None
            }
        );
        let response = stream.finish().unwrap();
        assert_eq!(response.usage.web_searches, Some(1));
        let tokens = catalog().model(MODEL).unwrap().cost(&TokenUsage { web_searches: None, ..response.usage.clone() }, None).usd.unwrap();
        assert!((response.cost.usd.unwrap() - tokens - 0.01).abs() < 1e-9, "一次搜索 $0.01 算进花费");
        assert!(matches!(&response.message.parts[..], [Part::ServerTool { .. }, Part::ServerTool { .. }, Part::Text { .. }]));

        // 回放：同一个模型原样带回（调用带上流进来的参数、结果带密文）；换了模型整块丢掉。
        let blocks = assistant_blocks(&response.message, &origin_for(MODEL).unwrap());
        assert_eq!(blocks[0]["input"], json!({"query": "tauri 2 release"}));
        assert_eq!(blocks[1]["content"][0]["encrypted_content"], "EqgfCio");
        let blocks = assistant_blocks(&response.message, &origin_for("anthropic/claude-opus-5-5").unwrap());
        assert_eq!(blocks.len(), 1, "换了模型只剩文字");

        // pause_turn：搜到一半先停；出错的结果报原因。
        let mut paused = AnthropicStream::new(origin_for(MODEL).unwrap(), Some(MODEL.into()));
        for l in [
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "output_tokens": 1}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "server_tool_use", "id": "srvtoolu_2", "name": "web_search", "input": {"query": "x"}}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_2", "content": {"type": "web_search_tool_result_error", "error_code": "max_uses_exceeded"}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "pause_turn"}, "usage": {"output_tokens": 5}}),
            json!({"type": "message_stop"}),
        ] {
            for e in paused.feed(&l) {
                if let ModelEvent::ServerToolDone { error, query, .. } = e {
                    assert_eq!((error.as_deref(), query.as_deref()), (Some("这一轮搜的次数到上限了"), Some("x")));
                }
            }
        }
        assert_eq!(paused.finish().unwrap().stop, StopReason::Paused);
    }
}
