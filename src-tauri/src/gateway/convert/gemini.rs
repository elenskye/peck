//! Gemini 原生 `streamGenerateContent?alt=sse`：统一的请求 → body，流 → 增量和整条回答。
//! Gemini（Key）和 Vertex 上的 Gemini（ADC）共用，只有地址和认证不同。
//!
//! thought signature 是回放的关键：调用工具的那一段必须原样带回，不然 400。
//! - 挂在工具调用上的：放进 `Part::ToolCall.signature`。
//! - 挂在文字或思考上的：那一块后面跟一个空文字的 `Part::Reasoning`，私有数据是
//!   `{"thoughtSignature": …, "on": "previous"}`，回放时贴回前一块。界面不显示空的思考。
//!
//! 流里一块常被拆成好几段（同一种文字接着来），这里拼回去；签名跟着它到的那一块走。
//! Gemini 不总给工具调用 id，没给的这里编一个 `peck-call-<n>`，回放时不发。

use super::{minus, priced, system_text, tool_names, WireStream};
use crate::gateway::catalog::{ModelSpec, Thinking};
use crate::gateway::event::{OpaqueReasoning, Origin, TokenUsage};
use crate::gateway::{Message, ModelEvent, ModelRequest, ModelResponse, Part, Role, StopReason};
use serde_json::{json, Value};

const MADE_UP_ID: &str = "peck-call-";

/// 统一的请求 → Gemini 的 body。
pub fn encode(request: &ModelRequest, model: &ModelSpec, origin: &Origin) -> Value {
    let names = tool_names(request);
    let mut contents = Vec::new();
    for message in &request.messages {
        let parts = match message.role {
            Role::User => user_parts(message, &names),
            Role::Assistant => model_parts(message, origin),
        };
        if !parts.is_empty() {
            let role = if message.role == Role::User { "user" } else { "model" };
            contents.push(json!({"role": role, "parts": parts}));
        }
    }
    let mut config = json!({"maxOutputTokens": request.max_output_tokens});
    let on = match model.thinking {
        Thinking::Always => true,
        Thinking::Switch => request.thinking,
        Thinking::Never => false,
    };
    if on {
        let mut thinking = json!({"includeThoughts": true});
        if let Some(effort) = &request.effort {
            thinking["thinkingLevel"] = json!(effort);
        }
        config["thinkingConfig"] = thinking;
    } else if model.thinking == Thinking::Switch {
        config["thinkingConfig"] = json!({"thinkingBudget": 0});
    }
    let mut body = json!({"contents": contents, "generationConfig": config});
    let system = system_text(request);
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system}]});
    }
    if !request.tools.is_empty() {
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "parametersJsonSchema": t.input_schema}))
            .collect();
        body["tools"] = json!([{"functionDeclarations": declarations}]);
    }
    body
}

fn user_parts(message: &Message, names: &std::collections::HashMap<String, String>) -> Vec<Value> {
    let mut results = Vec::new();
    let mut texts = Vec::new();
    for part in &message.parts {
        match part {
            Part::ToolResult {
                call_id,
                output,
                is_error,
            } => {
                let name = names.get(call_id).cloned().unwrap_or_default();
                let response = if *is_error {
                    json!({"error": output})
                } else {
                    json!({"output": output})
                };
                let mut r = json!({"name": name, "response": response});
                if !call_id.starts_with(MADE_UP_ID) {
                    r["id"] = json!(call_id);
                }
                results.push(json!({"functionResponse": r}));
            }
            Part::Text { text } => texts.push(json!({"text": text})),
            Part::Reasoning { .. } | Part::ToolCall { .. } => {}
        }
    }
    results.extend(texts);
    results
}

fn model_parts(message: &Message, origin: &Origin) -> Vec<Value> {
    let same = |o: &Option<OpaqueReasoning>| -> Option<Value> {
        o.as_ref()
            .filter(|o| o.origin.can_replay_to(origin))
            .and_then(|o| o.data.get("thoughtSignature").cloned())
    };
    let mut out: Vec<Value> = Vec::new();
    for part in &message.parts {
        match part {
            Part::Reasoning { text, opaque } => {
                let is_attached = opaque.as_ref().is_some_and(|o| o.data["on"] == "previous");
                let Some(o) = opaque.as_ref().filter(|o| o.origin.can_replay_to(origin)) else {
                    continue;
                };
                if is_attached {
                    if let (Some(last), Some(sig)) = (out.last_mut(), o.data.get("thoughtSignature")) {
                        last["thoughtSignature"] = sig.clone();
                    }
                    continue;
                }
                let mut p = json!({"text": text, "thought": true});
                if let Some(sig) = o.data.get("thoughtSignature") {
                    p["thoughtSignature"] = sig.clone();
                }
                out.push(p);
            }
            Part::Text { text } => out.push(json!({"text": text})),
            Part::ToolCall {
                call_id,
                name,
                input,
                signature,
            } => {
                let mut call = json!({"name": name, "args": input});
                if !call_id.starts_with(MADE_UP_ID) {
                    call["id"] = json!(call_id);
                }
                let mut p = json!({"functionCall": call});
                if let Some(sig) = same(signature) {
                    p["thoughtSignature"] = sig;
                }
                out.push(p);
            }
            Part::ToolResult { .. } => {}
        }
    }
    out
}

enum Piece {
    Thought { text: String, signature: Option<String> },
    Text { text: String, signature: Option<String> },
    Call { id: String, name: String, args: Value, signature: Option<String> },
}

pub struct GeminiStream {
    origin: Origin,
    catalog_id: Option<String>,
    pieces: Vec<Piece>,
    usage: TokenUsage,
    finish: Option<String>,
    blocked: Option<String>,
    response_model: Option<String>,
    error: Option<String>,
    calls: usize,
}

impl GeminiStream {
    pub fn new(origin: Origin, catalog_id: Option<String>) -> Self {
        Self {
            origin,
            catalog_id,
            pieces: Vec::new(),
            usage: TokenUsage::default(),
            finish: None,
            blocked: None,
            response_model: None,
            error: None,
            calls: 0,
        }
    }

    fn opaque(&self, data: Value) -> Option<OpaqueReasoning> {
        Some(OpaqueReasoning {
            origin: self.origin.clone(),
            data,
        })
    }
}

impl WireStream for GeminiStream {
    fn feed(&mut self, data: &Value) -> Vec<ModelEvent> {
        if let Some(e) = data.get("error").filter(|e| !e.is_null()) {
            self.error = Some(match e["message"].as_str() {
                Some(m) => format!("供应商在回答中途报错：{}", super::short(m, 160)),
                None => "供应商在回答中途报错。".into(),
            });
            return vec![];
        }
        if let Some(m) = data["modelVersion"].as_str() {
            self.response_model = Some(m.into());
        }
        if let Some(reason) = data["promptFeedback"]["blockReason"].as_str() {
            self.blocked = Some(reason.into());
        }
        let u = &data["usageMetadata"];
        if u.is_object() {
            let cached = u["cachedContentTokenCount"].as_u64();
            let thoughts = u["thoughtsTokenCount"].as_u64();
            self.usage.input = minus(u["promptTokenCount"].as_u64(), cached);
            self.usage.cache_read = cached;
            self.usage.reasoning = thoughts;
            self.usage.output = match (u["candidatesTokenCount"].as_u64(), thoughts) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            };
        }
        let mut out = Vec::new();
        let candidate = &data["candidates"][0];
        for p in candidate["content"]["parts"].as_array().into_iter().flatten() {
            let signature = p["thoughtSignature"].as_str().map(str::to_string);
            if let Some(call) = p.get("functionCall") {
                self.calls += 1;
                let id = call["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{MADE_UP_ID}{}", self.calls));
                let name = call["name"].as_str().unwrap_or("").to_string();
                out.push(ModelEvent::ToolCallStarted {
                    call_id: id.clone(),
                    name: name.clone(),
                });
                self.pieces.push(Piece::Call {
                    id,
                    name,
                    args: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    signature,
                });
                continue;
            }
            let Some(text) = p["text"].as_str() else {
                // 没文字只有签名的块：签名贴到前一块（前一块还没签名时）。
                if let (Some(sig), Some(last)) = (signature, self.pieces.last_mut()) {
                    if signature_of(last).is_none() {
                        set_signature(last, sig);
                    }
                }
                continue;
            };
            let thought = p["thought"].as_bool().unwrap_or(false);
            if text.is_empty() && signature.is_none() {
                continue;
            }
            let last_unsigned = self.pieces.last().is_some_and(|l| signature_of(l).is_none());
            let extend = match self.pieces.last_mut() {
                Some(Piece::Thought { text: t, signature: None }) if thought => Some(t),
                Some(Piece::Text { text: t, signature: None }) if !thought => Some(t),
                _ => None,
            };
            match extend {
                Some(t) => t.push_str(text),
                // 只带签名的空文字：贴到前一块（前一块还没签名时）。
                None if text.is_empty() && last_unsigned => {}
                None => self.pieces.push(if thought {
                    Piece::Thought { text: text.into(), signature: None }
                } else {
                    Piece::Text { text: text.into(), signature: None }
                }),
            }
            if let (Some(sig), Some(last)) = (signature, self.pieces.last_mut()) {
                set_signature(last, sig);
            }
            if !text.is_empty() {
                out.push(if thought {
                    ModelEvent::ReasoningDelta { text: text.into() }
                } else {
                    ModelEvent::TextDelta { text: text.into() }
                });
            }
        }
        if let Some(f) = candidate["finishReason"].as_str() {
            self.finish = Some(f.into());
        }
        out
    }

    fn finish(self: Box<Self>) -> Result<ModelResponse, String> {
        let this = *self;
        if let Some(e) = this.error.clone() {
            return Err(e);
        }
        if this.finish.is_none() && this.blocked.is_none() {
            return Err("回答在结束前断开了，没自动重发。".into());
        }
        let mut parts = Vec::new();
        let mut has_call = false;
        for piece in &this.pieces {
            match piece {
                Piece::Thought { text, signature } => parts.push(Part::Reasoning {
                    text: text.clone(),
                    opaque: this.opaque(match signature {
                        Some(sig) => json!({"thoughtSignature": sig}),
                        None => json!({}),
                    }),
                }),
                Piece::Text { text, signature } => {
                    parts.push(Part::Text { text: text.clone() });
                    if let Some(sig) = signature {
                        parts.push(Part::Reasoning {
                            text: String::new(),
                            opaque: this.opaque(json!({"thoughtSignature": sig, "on": "previous"})),
                        });
                    }
                }
                Piece::Call {
                    id,
                    name,
                    args,
                    signature,
                } => {
                    has_call = true;
                    parts.push(Part::ToolCall {
                        call_id: id.clone(),
                        name: name.clone(),
                        input: args.clone(),
                        signature: signature
                            .as_ref()
                            .and_then(|sig| this.opaque(json!({"thoughtSignature": sig}))),
                    });
                }
            }
        }
        let stop = if this.blocked.is_some() {
            StopReason::Refusal
        } else {
            match this.finish.as_deref().unwrap_or("") {
                "STOP" if has_call => StopReason::ToolUse,
                "STOP" | "FINISH_REASON_UNSPECIFIED" => StopReason::EndTurn,
                "MAX_TOKENS" => StopReason::MaxTokens,
                "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" | "IMAGE_SAFETY" => {
                    StopReason::Refusal
                }
                other => StopReason::Other(other.to_lowercase()),
            }
        };
        let cost = priced(this.catalog_id.as_deref(), None, &this.usage);
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

fn signature_of(piece: &Piece) -> Option<&String> {
    match piece {
        Piece::Thought { signature, .. } | Piece::Text { signature, .. } | Piece::Call { signature, .. } => {
            signature.as_ref()
        }
    }
}

fn set_signature(piece: &mut Piece, sig: String) {
    match piece {
        Piece::Thought { signature, .. } | Piece::Text { signature, .. } | Piece::Call { signature, .. } => {
            *signature = Some(sig)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::catalog::catalog;
    use crate::gateway::event::Wire;
    use crate::gateway::ToolSpec;

    const MODEL: &str = "google/gemini-3.5-flash-lite";

    fn origin() -> Origin {
        Origin {
            provider: "google".into(),
            wire: Wire::Gemini,
            model: "gemini-3.5-flash-lite".into(),
        }
    }

    fn chunk(parts: Value, finish: Option<&str>) -> Value {
        let mut c = json!({"candidates": [{"content": {"role": "model", "parts": parts}}], "modelVersion": "gemini-3.5-flash-lite"});
        if let Some(f) = finish {
            c["candidates"][0]["finishReason"] = json!(f);
        }
        c
    }

    #[test]
    fn split_pieces_join_and_signatures_go_back_where_they_came_from() {
        let mut s: Box<dyn WireStream> = Box::new(GeminiStream::new(origin(), Some(MODEL.into())));
        let mut events = Vec::new();
        for line in [
            chunk(json!([{"text": "先读", "thought": true}]), None),
            chunk(json!([{"text": "文件", "thought": true}]), None),
            chunk(json!([{"functionCall": {"name": "read_file", "args": {"path": "a"}}, "thoughtSignature": "SIG1"}]), None),
            chunk(json!([{"text": ""}]), Some("STOP")),
        ] {
            events.extend(s.feed(&line));
        }
        let mut last = chunk(json!([]), None);
        last["usageMetadata"] = json!({"promptTokenCount": 50, "cachedContentTokenCount": 10, "candidatesTokenCount": 8, "thoughtsTokenCount": 20});
        s.feed(&last);
        let r = s.finish().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(r.stop, StopReason::ToolUse);
        assert_eq!((r.usage.input, r.usage.cache_read, r.usage.output, r.usage.reasoning), (Some(40), Some(10), Some(28), Some(20)));
        let Part::ToolCall { call_id, signature, .. } = &r.message.parts[1] else { panic!("第二块是调用") };
        assert!(call_id.starts_with(MADE_UP_ID));
        assert_eq!(signature.as_ref().unwrap().data["thoughtSignature"], "SIG1");

        let model = catalog().model(MODEL).unwrap();
        let request = ModelRequest {
            model: MODEL.into(),
            system: vec![],
            messages: vec![
                Message { role: Role::User, parts: vec![Part::Text { text: "读 a".into() }] },
                r.message.clone(),
                Message { role: Role::User, parts: vec![Part::ToolResult { call_id: call_id.clone(), output: "内容".into(), is_error: false }] },
            ],
            tools: vec![ToolSpec { name: "read_file".into(), description: "读".into(), input_schema: json!({"type": "object"}) }],
            thinking: true,
            effort: Some("low".into()),
            max_output_tokens: 800,
        };
        let body = encode(&request, model, &origin());
        let c = &body["contents"];
        assert_eq!(c[1]["role"], "model");
        assert_eq!(c[1]["parts"][0], json!({"text": "先读文件", "thought": true}));
        assert_eq!(c[1]["parts"][1], json!({"functionCall": {"name": "read_file", "args": {"path": "a"}}, "thoughtSignature": "SIG1"}));
        assert_eq!(c[2]["parts"][0], json!({"functionResponse": {"name": "read_file", "response": {"output": "内容"}}}));
        assert_eq!(body["generationConfig"]["thinkingConfig"], json!({"includeThoughts": true, "thinkingLevel": "low"}));
        assert!(body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"].is_object());

        // 换一家：签名不带。
        let other = Origin { model: "gemini-3.8-flash".into(), ..origin() };
        let body = encode(&request, model, &other);
        assert!(body["contents"][1]["parts"][0].get("thoughtSignature").is_none());
        assert!(body["contents"][1]["parts"].as_array().unwrap().iter().all(|p| p.get("thoughtSignature").is_none()));
    }

    #[test]
    fn text_signature_rides_on_an_empty_reasoning_and_comes_back() {
        let mut s: Box<dyn WireStream> = Box::new(GeminiStream::new(origin(), None));
        s.feed(&chunk(json!([{"text": "一共"}]), None));
        s.feed(&chunk(json!([{"text": "三条。"}]), None));
        s.feed(&chunk(json!([{"text": "", "thoughtSignature": "SIG2"}]), Some("STOP")));
        let r = s.finish().unwrap();
        assert_eq!(r.stop, StopReason::EndTurn);
        assert_eq!(r.message.parts.len(), 2);
        assert_eq!(r.message.parts[0], Part::Text { text: "一共三条。".into() });
        let parts = model_parts(&r.message, &origin());
        assert_eq!(parts, vec![json!({"text": "一共三条。", "thoughtSignature": "SIG2"})]);
    }

    #[test]
    fn blocked_prompts_and_cut_streams() {
        let mut s: Box<dyn WireStream> = Box::new(GeminiStream::new(origin(), None));
        s.feed(&json!({"promptFeedback": {"blockReason": "SAFETY"}}));
        assert_eq!(s.finish().unwrap().stop, StopReason::Refusal);
        let mut s: Box<dyn WireStream> = Box::new(GeminiStream::new(origin(), None));
        s.feed(&chunk(json!([{"text": "半"}]), None));
        assert!(s.finish().is_err());
    }
}
