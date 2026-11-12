//! Chat Completions（流式 SSE），DeepSeek 和百炼两种方言。
//!
//! - DeepSeek：思考开关是 `thinking: {type}`，强度 `reasoning_effort`；调用工具的那一轮里
//!   `reasoning_content` 要原样放回（来源一样才放）。缓存命中在 `prompt_cache_hit_tokens`。
//! - 百炼（Qwen）：开关是 `enable_thinking`；历史里的 `reasoning_content` 不放回（百炼不读它）。
//!   缓存命中在 `prompt_tokens_details.cached_tokens`。

use super::{minus, priced, system_text, WireStream};
use crate::gateway::catalog::{ModelSpec, Thinking};
use crate::gateway::event::{OpaqueReasoning, Origin, TokenUsage};
use crate::gateway::{Message, ModelEvent, ModelRequest, ModelResponse, Part, Role, StopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    DeepSeek,
    Qwen,
}

impl Dialect {
    pub fn from_catalog(dialect: Option<&str>) -> Self {
        match dialect {
            Some("qwen") => Self::Qwen,
            _ => Self::DeepSeek,
        }
    }
}

/// 统一的请求 → Chat Completions 的 body。
pub fn encode(request: &ModelRequest, model: &ModelSpec, origin: &Origin, dialect: Dialect) -> Value {
    let mut messages = Vec::new();
    let system = system_text(request);
    if !system.is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        match message.role {
            Role::User => {
                let mut texts = Vec::new();
                for part in &message.parts {
                    match part {
                        Part::ToolResult {
                            call_id, output, ..
                        } => messages.push(json!({"role": "tool", "tool_call_id": call_id, "content": output})),
                        Part::Text { text } => texts.push(text.as_str()),
                        _ => {}
                    }
                }
                if !texts.is_empty() {
                    messages.push(json!({"role": "user", "content": texts.join("\n\n")}));
                }
            }
            Role::Assistant => messages.push(assistant_message(message, origin, dialect)),
        }
    }
    let mut body = json!({
        "model": model.api_model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
        "max_tokens": request.max_output_tokens,
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.input_schema}}))
            .collect();
        body["tools"] = Value::Array(tools);
    }
    let on = match model.thinking {
        Thinking::Always => true,
        Thinking::Switch => request.thinking,
        Thinking::Never => false,
    };
    match dialect {
        Dialect::DeepSeek => {
            if model.thinking != Thinking::Never {
                body["thinking"] = json!({"type": if on { "enabled" } else { "disabled" }});
            }
            if on {
                if let Some(effort) = &request.effort {
                    body["reasoning_effort"] = json!(effort);
                }
            }
        }
        Dialect::Qwen => {
            if model.thinking == Thinking::Switch {
                body["enable_thinking"] = json!(on);
            }
        }
    }
    body
}

fn assistant_message(message: &Message, origin: &Origin, dialect: Dialect) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for part in &message.parts {
        match part {
            Part::Text { text: t } => text.push_str(t),
            Part::Reasoning { text: t, opaque } => {
                if dialect == Dialect::DeepSeek
                    && opaque.as_ref().is_some_and(|o| o.origin.can_replay_to(origin))
                {
                    reasoning.push_str(t);
                }
            }
            Part::ToolCall {
                call_id,
                name,
                input,
                ..
            } => calls.push(json!({"id": call_id, "type": "function", "function": {"name": name, "arguments": input.to_string()}})),
            Part::ToolResult { .. } => {}
        }
    }
    let mut m = json!({"role": "assistant", "content": text});
    if !reasoning.is_empty() {
        m["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        m["tool_calls"] = Value::Array(calls);
    }
    m
}

struct Call {
    id: String,
    name: String,
    arguments: String,
    announced: bool,
}

pub struct ChatStream {
    origin: Origin,
    catalog_id: Option<String>,
    region: Option<String>,
    dialect: Dialect,
    text: String,
    reasoning: String,
    calls: BTreeMap<u64, Call>,
    usage: TokenUsage,
    finish: Option<String>,
    response_model: Option<String>,
    error: Option<String>,
}

impl ChatStream {
    pub fn new(origin: Origin, catalog_id: Option<String>, region: Option<String>, dialect: Dialect) -> Self {
        Self {
            origin,
            catalog_id,
            region,
            dialect,
            text: String::new(),
            reasoning: String::new(),
            calls: BTreeMap::new(),
            usage: TokenUsage::default(),
            finish: None,
            response_model: None,
            error: None,
        }
    }

    fn take_usage(&mut self, u: &Value) {
        if !u.is_object() {
            return;
        }
        let prompt = u["prompt_tokens"].as_u64();
        match self.dialect {
            Dialect::DeepSeek => {
                let hit = u["prompt_cache_hit_tokens"].as_u64();
                self.usage.cache_read = hit;
                self.usage.input = u["prompt_cache_miss_tokens"].as_u64().or_else(|| minus(prompt, hit));
            }
            Dialect::Qwen => {
                let details = &u["prompt_tokens_details"];
                let hit = details["cached_tokens"].as_u64();
                let created = details["cache_creation_input_tokens"].as_u64();
                self.usage.cache_read = hit;
                self.usage.cache_write = created;
                self.usage.input = minus(minus(prompt, hit), created);
            }
        }
        self.usage.output = u["completion_tokens"].as_u64();
        self.usage.reasoning = u["completion_tokens_details"]["reasoning_tokens"].as_u64();
    }
}

impl WireStream for ChatStream {
    fn feed(&mut self, data: &Value) -> Vec<ModelEvent> {
        if let Some(e) = data.get("error").filter(|e| !e.is_null()) {
            self.error = Some(match e["message"].as_str() {
                Some(m) => format!("供应商在回答中途报错：{}", super::short(m, 160)),
                None => "供应商在回答中途报错。".into(),
            });
            return vec![];
        }
        if let Some(m) = data["model"].as_str() {
            self.response_model = Some(m.into());
        }
        self.take_usage(&data["usage"]);
        let mut out = Vec::new();
        for choice in data["choices"].as_array().into_iter().flatten() {
            let d = &choice["delta"];
            if let Some(t) = d["reasoning_content"].as_str().filter(|t| !t.is_empty()) {
                self.reasoning.push_str(t);
                out.push(ModelEvent::ReasoningDelta { text: t.into() });
            }
            if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
                self.text.push_str(t);
                out.push(ModelEvent::TextDelta { text: t.into() });
            }
            for (n, call) in d["tool_calls"].as_array().into_iter().flatten().enumerate() {
                let index = call["index"].as_u64().unwrap_or(n as u64);
                let entry = self.calls.entry(index).or_insert_with(|| Call {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                    announced: false,
                });
                if let Some(id) = call["id"].as_str().filter(|s| !s.is_empty()) {
                    entry.id = id.into();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    entry.name.push_str(name);
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    entry.arguments.push_str(args);
                }
                if !entry.announced && !entry.id.is_empty() && !entry.name.is_empty() {
                    entry.announced = true;
                    out.push(ModelEvent::ToolCallStarted {
                        call_id: entry.id.clone(),
                        name: entry.name.clone(),
                    });
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                self.finish = Some(f.into());
            }
        }
        out
    }

    fn finish(self: Box<Self>) -> Result<ModelResponse, String> {
        let this = *self;
        if let Some(e) = this.error {
            return Err(e);
        }
        let Some(finish) = this.finish else {
            return Err("回答在结束前断开了，没自动重发。".into());
        };
        let mut parts = Vec::new();
        if !this.reasoning.is_empty() {
            parts.push(Part::Reasoning {
                text: this.reasoning,
                opaque: Some(OpaqueReasoning {
                    origin: this.origin.clone(),
                    data: json!({"type": "reasoning_content"}),
                }),
            });
        }
        if !this.text.is_empty() {
            parts.push(Part::Text { text: this.text });
        }
        for call in this.calls.into_values() {
            if call.id.is_empty() || call.name.is_empty() {
                return Err("工具调用的数据不完整，没执行。".into());
            }
            let input = if call.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&call.arguments)
                    .map_err(|_| format!("工具 {} 的参数不完整（可能被截断）", call.name))?
            };
            parts.push(Part::ToolCall {
                call_id: call.id,
                name: call.name,
                input,
                signature: None,
            });
        }
        let stop = match finish.as_str() {
            "stop" => StopReason::EndTurn,
            "tool_calls" | "function_call" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            "content_filter" | "insufficient_system_resource" => StopReason::Refusal,
            other => StopReason::Other(other.into()),
        };
        let cost = priced(this.catalog_id.as_deref(), this.region.as_deref(), &this.usage);
        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                parts,
            },
            stop,
            usage: this.usage,
            cost,
            response_model: this.response_model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::catalog::catalog;
    use crate::gateway::event::Wire;

    fn origin(provider: &str, model: &str) -> Origin {
        Origin {
            provider: provider.into(),
            wire: Wire::Chat,
            model: model.into(),
        }
    }

    #[test]
    fn deepseek_reasoning_is_replayed_and_tool_fragments_join() {
        let o = origin("deepseek", "deepseek-flash");
        let mut s: Box<dyn WireStream> =
            Box::new(ChatStream::new(o.clone(), Some("deepseek/deepseek-flash".into()), None, Dialect::DeepSeek));
        let mut events = Vec::new();
        for line in [
            json!({"model": "deepseek-flash", "choices": [{"index": 0, "delta": {"role": "assistant", "reasoning_content": "要读"}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "read_file", "arguments": "{\"pa"}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "th\":\"a\"}"}}]}, "finish_reason": "tool_calls"}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 100, "prompt_cache_hit_tokens": 64, "prompt_cache_miss_tokens": 36, "completion_tokens": 20, "completion_tokens_details": {"reasoning_tokens": 12}}}),
        ] {
            events.extend(s.feed(&line));
        }
        let r = s.finish().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!((r.usage.input, r.usage.cache_read, r.usage.output), (Some(36), Some(64), Some(20)));
        assert!(matches!(&r.message.parts[1], Part::ToolCall { input, .. } if input["path"] == "a"));

        let model = catalog().model("deepseek/deepseek-flash").unwrap();
        let request = ModelRequest {
            model: model.id.clone(),
            system: vec![],
            messages: vec![
                Message { role: Role::User, parts: vec![Part::Text { text: "读 a".into() }] },
                r.message.clone(),
                Message { role: Role::User, parts: vec![Part::ToolResult { call_id: "call_1".into(), output: "内容".into(), is_error: false }] },
            ],
            tools: vec![],
            thinking: true,
            effort: Some("high".into()),
            max_output_tokens: 500,
        };
        let body = encode(&request, model, &o, Dialect::DeepSeek);
        let m = &body["messages"];
        assert_eq!(m[1]["reasoning_content"], "要读");
        assert_eq!(m[1]["tool_calls"][0]["function"]["arguments"], "{\"path\":\"a\"}");
        assert_eq!(m[2], json!({"role": "tool", "tool_call_id": "call_1", "content": "内容"}));
        assert_eq!(body["thinking"], json!({"type": "enabled"}));
        assert_eq!(body["reasoning_effort"], "high");
        // 百炼：不放回思考，开关用 enable_thinking。
        let qwen = catalog().model("aliyun/qwen3.8-flash").unwrap();
        let body = encode(&request, qwen, &origin("aliyun", "qwen3.8-flash"), Dialect::Qwen);
        assert!(body["messages"][1].get("reasoning_content").is_none());
        assert_eq!(body["enable_thinking"], true);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn qwen_cache_usage_and_cut_stream() {
        let mut s: Box<dyn WireStream> = Box::new(ChatStream::new(
            origin("aliyun", "qwen3.8-flash"),
            Some("aliyun/qwen3.8-flash".into()),
            Some("singapore".into()),
            Dialect::Qwen,
        ));
        s.feed(&json!({"choices": [{"delta": {"content": "好"}, "finish_reason": "stop"}]}));
        s.feed(&json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 10, "prompt_tokens_details": {"cached_tokens": 600}}}));
        let r = s.finish().unwrap();
        assert_eq!((r.usage.input, r.usage.cache_read), (Some(400), Some(600)));
        // 新加坡价：0.15 / 0.47 / 缓存读 0.016
        let want = (400.0 * 0.15 + 10.0 * 0.47 + 600.0 * 0.016) / 1e6;
        assert!((r.cost.usd.unwrap() - want).abs() < 1e-12);

        let mut cut: Box<dyn WireStream> =
            Box::new(ChatStream::new(origin("deepseek", "x"), None, None, Dialect::DeepSeek));
        cut.feed(&json!({"choices": [{"delta": {"content": "半"}}]}));
        assert!(cut.finish().is_err());
    }
}
