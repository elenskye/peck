//! Anthropic Messages（流式 SSE）→ 裸模型的增量和整条回答。Anthropic 和 Vertex 上的 Claude 共用。
//!
//! 思考块（`thinking` 带 `signature`、`redacted_thinking`）原样挂进 `OpaqueReasoning`，
//! 回放时 [`assistant_blocks`] 只在来源一样时放回去，换了就丢。

use crate::gateway::catalog::catalog;
use crate::gateway::event::{OpaqueReasoning, Origin, TokenUsage, Wire};
use crate::gateway::{Message, ModelEvent, ModelResponse, Part, Role, StopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 一段 SSE 原文 → 每条 `data:` 的 JSON（`ping` 也在里面，转换时跳过）。
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
                    (Block::ToolUse { json, .. }, "input_json_delta") => {
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
            "error" => {
                self.error = data["error"]["message"].as_str().map(str::to_string);
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
        "end_turn" | "stop_sequence" | "pause_turn" => StopReason::EndTurn,
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
        }
    }
    out
}

/// 目录里的模型 → 回放来源（供应商 + Messages + 发给供应商的名字）。
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
}
