use super::profiles::{AgentProfile, ModelProfile, Protocol, Provider};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub text: String,
    pub reasoning: String,
    pub message: Value,
    pub usage: Usage,
    pub stop_reason: Option<String>,
    pub response_model: Option<String>,
}

#[derive(Debug)]
pub enum Failure {
    Cancelled,
    Message(String),
}
impl Failure {
    fn message(s: &str) -> Self {
        Self::Message(s.into())
    }
}

pub enum Delta {
    Text(String),
    Reasoning(String),
}

pub fn body(
    model: &ModelProfile,
    agent: &AgentProfile,
    protocol: &Protocol,
    mut messages: Vec<Value>,
    tools: Vec<Value>,
) -> Value {
    if *protocol == Protocol::Responses {
        let input: Vec<Value> = messages
            .into_iter()
            .flat_map(|message| {
                message
                    .get("output")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_else(|| vec![message])
            })
            .collect();
        let mut body = json!({"model":model.api_model,"input":input,"stream":true,"store":false,"max_output_tokens":agent.max_output_tokens});
        if agent.thinking {
            body["reasoning"] = json!({"summary":"auto"});
            if let Some(effort) = &agent.effort {
                body["reasoning"]["effort"] = json!(effort);
            }
        } else if model.thinking == super::profiles::Thinking::Switch {
            body["reasoning"] = json!({"effort":"none"});
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        return body;
    }
    let system = if *protocol == Protocol::Anthropic
        && messages.first().is_some_and(|m| m["role"] == "system")
    {
        Some(messages.remove(0)["content"].clone())
    } else {
        None
    };
    let mut body = json!({"model":model.api_model,"stream":true,"messages":messages,"max_tokens":agent.max_output_tokens});
    if let Some(system) = system {
        body["system"] = system;
    }
    match protocol {
        Protocol::Anthropic => {
            body["thinking"] = if agent.thinking {
                json!({"type":"adaptive"})
            } else {
                json!({"type":"disabled"})
            };
            if let Some(effort) = &agent.effort {
                body["output_config"] = json!({"effort":effort});
            }
        }
        Protocol::ChatCompletions => {
            body["stream_options"] = json!({"include_usage":true});
            body["thinking"] = json!({"type":if agent.thinking {"enabled"} else {"disabled"}});
            if agent.thinking {
                if let Some(effort) = &agent.effort {
                    body["reasoning_effort"] = json!(effort);
                }
            }
        }
        Protocol::Gemini => {
            body["stream_options"] = json!({"include_usage":true});
            if agent.thinking {
                if let Some(effort) = &agent.effort {
                    body["reasoning_effort"] = json!(effort);
                }
            } else if model.thinking == super::profiles::Thinking::Switch {
                body["reasoning_effort"] = json!("none");
            }
        }
        Protocol::Qwen => {
            body["stream_options"] = json!({"include_usage":true});
            if model.thinking == super::profiles::Thinking::Switch {
                body["enable_thinking"] = json!(agent.thinking);
            }
        }
        Protocol::Responses => unreachable!(),
    }
    // Tools/messages are protocol-native; signed thinking and tool IDs stay intact on replay.
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    body
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|_| "无法创建 API 连接。".into())
}

pub async fn stream(
    client: &reqwest::Client,
    provider: &Provider,
    key: &str,
    body: Value,
    cancel: &mut watch::Receiver<bool>,
    mut emit: impl FnMut(Delta),
) -> Result<Response, Failure> {
    if *cancel.borrow() {
        return Err(Failure::Cancelled);
    }
    let (url, request) = match provider.protocol {
        Protocol::Anthropic => (format!("{}/messages", provider.base_url), true),
        Protocol::Responses => (format!("{}/responses", provider.base_url), false),
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
            (format!("{}/chat/completions", provider.base_url), false)
        }
    };
    let mut req = client
        .post(url)
        .header("Accept", "text/event-stream")
        .json(&body);
    if request {
        req = req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.bearer_auth(key);
    }
    let mut response = tokio::select! {
        biased;
        _=cancel.changed()=>return Err(Failure::Cancelled),
        result=req.send()=>result.map_err(|e|Failure::message(if e.is_timeout(){"API 请求超时，未自动重试。"}else{"API 网络连接失败，未自动重试。"}))?,
    };
    let status = response.status().as_u16();
    if status != 200 {
        return Err(Failure::Message(match status {
            401 | 403 => "认证或模型访问被拒绝，请检查 API Key 与账户权限。".into(),
            402 => "API 账户余额不足。".into(),
            429 => "API 额度或速率限制，请查看供应商账户；本次未自动重试。".into(),
            400 => "供应商拒绝请求参数；请检查模型与思考配置，未自动降档重发。".into(),
            _ => format!("供应商返回 HTTP {status}，未自动重试。"),
        }));
    }
    if !response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.starts_with("text/event-stream"))
    {
        return Err(Failure::message("供应商未返回预期的流式响应。"));
    }
    let mut decoder = Decoder::new(provider.protocol.clone());
    loop {
        let chunk = tokio::select! {
            biased;
            _=cancel.changed()=>return Err(Failure::Cancelled),
            result=response.chunk()=>result.map_err(|e|Failure::message(if e.is_timeout(){"读取 API 响应超时，已停止。"}else{"API 响应中断，已保留接收到的文本；未自动重发。"}))?,
        };
        match chunk {
            Some(chunk) => decoder.feed(&chunk, &mut emit).map_err(Failure::Message)?,
            None => break,
        }
        if decoder.done {
            break;
        }
    }
    decoder.finish().map_err(Failure::Message)
}

// Byte framing avoids corrupting UTF-8 when a network packet splits a Chinese character.
struct Decoder {
    protocol: Protocol,
    buffer: Vec<u8>,
    data: Vec<String>,
    done: bool,
    response: Response,
    blocks: BTreeMap<usize, Value>,
    arguments: BTreeMap<usize, String>,
    tools: BTreeMap<usize, Value>,
    extra_content: Option<Value>,
}
impl Decoder {
    fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            buffer: vec![],
            data: vec![],
            done: false,
            response: Response {
                text: String::new(),
                reasoning: String::new(),
                message: Value::Null,
                usage: Usage::default(),
                stop_reason: None,
                response_model: None,
            },
            blocks: BTreeMap::new(),
            arguments: BTreeMap::new(),
            tools: BTreeMap::new(),
            extra_content: None,
        }
    }

    fn feed(&mut self, bytes: &[u8], emit: &mut impl FnMut(Delta)) -> Result<(), String> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > 4 * 1024 * 1024 {
            return Err("流式响应单帧过大，已停止。".into());
        }
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "API 响应编码无效。")?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    let data = self.data.join("\n");
                    self.data.clear();
                    self.event(&data, emit)?;
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).into());
            }
            if self.data.iter().map(String::len).sum::<usize>() > 4 * 1024 * 1024 {
                return Err("流式响应单帧过大，已停止。".into());
            }
        }
        if self.response.text.len() + self.response.reasoning.len() > 16 * 1024 * 1024 {
            return Err("API 响应超过本次大小上限。".into());
        }
        Ok(())
    }

    fn event(&mut self, data: &str, emit: &mut impl FnMut(Delta)) -> Result<(), String> {
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let v: Value = serde_json::from_str(data).map_err(|_| "API 返回了无效的流式 JSON。")?;
        if v.get("error").is_some() || v["type"] == "error" {
            return Err("供应商在响应过程中报告错误，未自动重试。".into());
        }
        match self.protocol {
            Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
                if let Some(model) = v["model"].as_str() {
                    self.response.response_model = Some(model.into());
                }
                self.usage(&v["usage"]);
                if let Some(choices) = v["choices"].as_array() {
                    for c in choices {
                        if let Some(stop) = c["finish_reason"].as_str() {
                            self.response.stop_reason = Some(stop.into());
                        }
                        let delta = &c["delta"];
                        if let Some(extra) = delta.get("extra_content") {
                            self.extra_content = Some(extra.clone());
                        }
                        if let Some(s) = delta["content"].as_str() {
                            self.response.text.push_str(s);
                            emit(Delta::Text(s.into()));
                        }
                        if let Some(s) = delta["reasoning_content"].as_str() {
                            self.response.reasoning.push_str(s);
                            emit(Delta::Reasoning(s.into()));
                        }
                        if let Some(tools) = delta["tool_calls"].as_array() {
                            for tool in tools {
                                let i =
                                    tool["index"].as_u64().ok_or("工具调用缺少索引。")? as usize;
                                let target=self.tools.entry(i).or_insert_with(||json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                                for field in ["id", "type"] {
                                    if let Some(value) = tool.get(field) {
                                        target[field] = value.clone();
                                    }
                                }
                                if let Some(extra) = tool.get("extra_content") {
                                    target["extra_content"] = extra.clone();
                                }
                                for field in ["name", "arguments"] {
                                    if let Some(s) = tool["function"][field].as_str() {
                                        let previous =
                                            target["function"][field].as_str().unwrap_or("");
                                        target["function"][field] = json!(format!("{previous}{s}"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Protocol::Responses => match v["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    if let Some(delta) = v["delta"].as_str() {
                        self.response.text.push_str(delta);
                        emit(Delta::Text(delta.into()));
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    if let Some(delta) = v["delta"].as_str() {
                        self.response.reasoning.push_str(delta);
                        emit(Delta::Reasoning(delta.into()));
                    }
                }
                "response.completed" | "response.incomplete" => {
                    let response = &v["response"];
                    let output = response["output"]
                        .as_array()
                        .ok_or("Responses 缺少 output。")?;
                    self.response.message = json!({"output":output});
                    self.response.response_model = response["model"].as_str().map(String::from);
                    self.response.text = output
                        .iter()
                        .flat_map(|item| item["content"].as_array().into_iter().flatten())
                        .filter(|item| item["type"] == "output_text")
                        .filter_map(|item| item["text"].as_str())
                        .collect::<String>();
                    self.response.reasoning = output
                        .iter()
                        .flat_map(|item| item["summary"].as_array().into_iter().flatten())
                        .filter_map(|item| item["text"].as_str())
                        .collect::<String>();
                    self.response.usage.input_tokens = response["usage"]["input_tokens"].as_u64();
                    self.response.usage.output_tokens = response["usage"]["output_tokens"].as_u64();
                    self.response.usage.cache_read_tokens =
                        response["usage"]["input_tokens_details"]["cached_tokens"].as_u64();
                    self.response.usage.reasoning_tokens =
                        response["usage"]["output_tokens_details"]["reasoning_tokens"].as_u64();
                    self.response.stop_reason = Some(
                        if v["type"] == "response.incomplete" {
                            response["incomplete_details"]["reason"]
                                .as_str()
                                .unwrap_or("incomplete")
                        } else if output.iter().any(|item| item["type"] == "function_call") {
                            "tool_calls"
                        } else {
                            "stop"
                        }
                        .into(),
                    );
                    self.done = true;
                }
                "response.failed" => return Err("OpenAI Responses 请求失败，未自动重试。".into()),
                _ => {}
            },
            Protocol::Anthropic => match v["type"].as_str().unwrap_or("") {
                "message_start" => {
                    self.response.message = v["message"].clone();
                    self.response.response_model = v["message"]["model"].as_str().map(String::from);
                    self.usage(&v["message"]["usage"]);
                }
                "content_block_start" => {
                    let i = v["index"].as_u64().ok_or("内容块缺少索引。")? as usize;
                    self.blocks.insert(i, v["content_block"].clone());
                }
                "content_block_delta" => {
                    let i = v["index"].as_u64().ok_or("内容块缺少索引。")? as usize;
                    let delta = &v["delta"];
                    let block = self.blocks.get_mut(&i).ok_or("内容块顺序无效。")?;
                    let field = match delta["type"].as_str().unwrap_or("") {
                        "text_delta" => Some("text"),
                        "thinking_delta" => Some("thinking"),
                        "signature_delta" => Some("signature"),
                        _ => None,
                    };
                    if let Some(field) = field {
                        if let Some(s) = delta[field].as_str() {
                            let previous = block[field].as_str().unwrap_or("");
                            block[field] = json!(format!("{previous}{s}"));
                            match field {
                                "text" => {
                                    self.response.text.push_str(s);
                                    emit(Delta::Text(s.into()));
                                }
                                "thinking" => {
                                    self.response.reasoning.push_str(s);
                                    emit(Delta::Reasoning(s.into()));
                                }
                                _ => {}
                            }
                        }
                    }
                    if let Some(s) = delta["partial_json"].as_str() {
                        self.arguments.entry(i).or_default().push_str(s);
                    }
                }
                "message_delta" => {
                    self.response.stop_reason =
                        v["delta"]["stop_reason"].as_str().map(String::from);
                    self.usage(&v["usage"]);
                    if let Some(fields) = v["delta"].as_object() {
                        for (k, value) in fields {
                            self.response.message[k] = value.clone();
                        }
                    }
                }
                "message_stop" => self.done = true,
                _ => {} // Includes ping and forward-compatible event types.
            },
        }
        Ok(())
    }

    fn usage(&mut self, v: &Value) {
        for (target, names) in [
            (
                &mut self.response.usage.input_tokens,
                ["input_tokens", "prompt_tokens"],
            ),
            (
                &mut self.response.usage.output_tokens,
                ["output_tokens", "completion_tokens"],
            ),
            (
                &mut self.response.usage.cache_read_tokens,
                ["cache_read_input_tokens", "prompt_cache_hit_tokens"],
            ),
            (
                &mut self.response.usage.cache_creation_tokens,
                ["cache_creation_input_tokens", "cache_creation_input_tokens"],
            ),
        ] {
            if let Some(n) = names.iter().find_map(|key| v[*key].as_u64()) {
                *target = Some(n);
            }
        }
        if let Some(n) = v["completion_tokens_details"]["reasoning_tokens"].as_u64() {
            self.response.usage.reasoning_tokens = Some(n);
        }
        if let Some(n) = v["prompt_tokens_details"]["cached_tokens"].as_u64() {
            self.response.usage.cache_read_tokens = Some(n);
        }
    }

    fn finish(mut self) -> Result<Response, String> {
        if !self.done {
            return Err("API 流在结束标记前断开；未自动重发。".into());
        }
        if self.response.stop_reason.is_none() {
            return Err("API 未返回完成原因。".into());
        }
        match self.protocol {
            Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
                for tool in self.tools.values() {
                    if tool["id"].as_str().unwrap_or("").is_empty()
                        || serde_json::from_str::<Value>(
                            tool["function"]["arguments"].as_str().unwrap_or(""),
                        )
                        .is_err()
                    {
                        return Err("工具调用数据不完整，未执行工具。".into());
                    }
                }
                let mut message = json!({"role":"assistant","content":self.response.text});
                if !self.response.reasoning.is_empty() {
                    message["reasoning_content"] = json!(self.response.reasoning);
                }
                if !self.tools.is_empty() {
                    message["tool_calls"] = json!(self.tools.into_values().collect::<Vec<_>>());
                }
                if let Some(extra) = self.extra_content {
                    message["extra_content"] = extra;
                }
                self.response.message = message;
            }
            Protocol::Anthropic => {
                if !self.response.message.is_object() {
                    return Err("API 缺少起始消息。".into());
                }
                for (i, raw) in self.arguments {
                    let parsed: Value =
                        serde_json::from_str(&raw).map_err(|_| "工具参数不完整，未执行工具。")?;
                    self.blocks.get_mut(&i).ok_or("工具块缺失。")?["input"] = parsed;
                }
                self.response.message = json!({"role":"assistant","content":self.blocks.into_values().collect::<Vec<_>>()});
            }
            Protocol::Responses => {
                for item in self.response.message["output"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if item["type"] == "function_call"
                        && (item["call_id"].as_str().unwrap_or("").is_empty()
                            || serde_json::from_str::<Value>(
                                item["arguments"].as_str().unwrap_or(""),
                            )
                            .is_err())
                    {
                        return Err("Responses 工具调用不完整，未执行工具。".into());
                    }
                }
            }
        }
        Ok(self.response)
    }
}

pub async fn mock_stream(
    protocol: Protocol,
    scenario: &str,
    cancel: &mut watch::Receiver<bool>,
    mut emit: impl FnMut(Delta),
) -> Result<Response, Failure> {
    let mut decoder = Decoder::new(protocol.clone());
    let text = "这是 API 模拟响应，不调用真实模型服务。";
    let events = match protocol {
        Protocol::Anthropic => vec![
            json!({"type":"message_start","message":{"role":"assistant","model":"mock","content":[],"usage":{"input_tokens":12,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":24}}),
            json!({"type":"message_stop"}),
        ],
        Protocol::Responses => vec![
            json!({"type":"response.output_text.delta","delta":text}),
            json!({"type":"response.completed","response":{"model":"mock","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}],"usage":{"input_tokens":12,"output_tokens":24}}}),
        ],
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => vec![
            json!({"model":"mock","choices":[{"delta":{"content":text},"finish_reason":null}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":24}}),
        ],
    };
    let mut frames = events
        .into_iter()
        .map(|v| format!("data: {v}\r\n\r\n"))
        .collect::<Vec<_>>();
    if matches!(
        protocol,
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen
    ) {
        frames.push("data: [DONE]\n\n".into());
    }
    for (i, frame) in frames.iter().enumerate() {
        if *cancel.borrow() {
            return Err(Failure::Cancelled);
        }
        tokio::select! { biased; _=cancel.changed()=>return Err(Failure::Cancelled), _=tokio::time::sleep(Duration::from_millis(if scenario=="slow" {2000}else{120}))=>{} }
        if i == 0 && scenario == "authentication" {
            return Err(Failure::message("模拟：API 认证失败。"));
        }
        if i == 0 && scenario == "rateLimit" {
            return Err(Failure::message("模拟：API 额度或速率限制；未自动重试。"));
        }
        if i == 1 && scenario == "disconnect" {
            return Err(Failure::message("模拟：API 网络中断；未自动重发。"));
        }
        for chunk in frame.as_bytes().chunks(17) {
            decoder.feed(chunk, &mut emit).map_err(Failure::Message)?;
        }
    }
    decoder.finish().map_err(Failure::Message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parameters_do_not_leak_between_dialects() {
        let (mut a, mut m, _) = super::super::profiles::Config::default()
            .selection("worker")
            .unwrap();
        m.api_model = "fixture-model".into();
        let google = body(&m, &a, &Protocol::Gemini, vec![], vec![]);
        assert_eq!(google["reasoning_effort"], "high");
        assert!(google.get("thinking").is_none());
        let qwen = body(&m, &a, &Protocol::Qwen, vec![], vec![]);
        assert_eq!(qwen["enable_thinking"], true);
        assert!(qwen.get("reasoning_effort").is_none());
        assert!(qwen.get("thinking").is_none());
        let openai = body(
            &m,
            &a,
            &Protocol::Responses,
            vec![json!({"role":"user","content":"task"})],
            vec![],
        );
        assert_eq!(openai["reasoning"]["effort"], "high");
        assert_eq!(openai["store"], false);
        assert_eq!(openai["input"][0]["content"], "task");
        assert!(openai.get("messages").is_none());
        assert!(openai.get("thinking").is_none());
        a.thinking = false;
        a.effort = None;
        assert_eq!(
            body(&m, &a, &Protocol::Qwen, vec![], vec![])["enable_thinking"],
            false
        );
        assert_eq!(
            body(&m, &a, &Protocol::Gemini, vec![], vec![])["reasoning_effort"],
            "none"
        );
        assert_eq!(
            body(&m, &a, &Protocol::Responses, vec![], vec![])["reasoning"]["effort"],
            "none"
        );
    }

    #[test]
    fn responses_stream_preserves_native_items_usage_and_tool_replay() {
        let output = json!([
            {"type":"reasoning","id":"rs_fixture","encrypted_content":"opaque-fixture","summary":[{"type":"summary_text","text":"概述"}]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"结果"}]},
            {"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"read_file","arguments":"{\"path\":\"a.txt\"}"}
        ]);
        let frames = [
            json!({"type":"response.output_text.delta","delta":"结果"}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"概述"}),
            json!({"type":"response.completed","response":{"model":"fixture-model","output":output,"usage":{"input_tokens":10,"output_tokens":4,"input_tokens_details":{"cached_tokens":2},"output_tokens_details":{"reasoning_tokens":2}}}}),
        ];
        let mut decoder = Decoder::new(Protocol::Responses);
        for frame in frames {
            for byte in format!("data: {frame}\n\n").as_bytes() {
                decoder.feed(&[*byte], &mut |_| {}).unwrap();
            }
        }
        let response = decoder.finish().unwrap();
        assert_eq!(response.text, "结果");
        assert_eq!(response.reasoning, "概述");
        assert_eq!(response.usage.input_tokens, Some(10));
        assert_eq!(response.usage.cache_read_tokens, Some(2));
        assert_eq!(response.usage.reasoning_tokens, Some(2));
        let call = super::super::tools::calls(&response.message, &Protocol::Responses)
            .unwrap()
            .remove(0);
        assert_eq!(call.id, "call_fixture");
        let mut messages = vec![response.message];
        super::super::tools::append_results(
            &mut messages,
            &Protocol::Responses,
            &[super::super::tools::ToolRecord {
                call_id: "request-fixture".into(),
                tool: call,
                status: "completed".into(),
                output: "file contents".into(),
                started_at: 1,
                finished_at: Some(2),
            }],
        );
        let (a, m, _) = super::super::profiles::Config::default()
            .selection("worker")
            .unwrap();
        let request = body(&m, &a, &Protocol::Responses, messages, vec![]);
        assert_eq!(request["input"][0]["encrypted_content"], "opaque-fixture");
        assert_eq!(request["input"][3]["call_id"], "call_fixture");
        assert_eq!(request["input"][3]["type"], "function_call_output");
        assert!(Decoder::new(Protocol::Responses).finish().is_err());
    }

    #[test]
    fn gemini_stream_keeps_message_and_tool_signatures() {
        let mut decoder = Decoder::new(Protocol::Gemini);
        for frame in [
            json!({"choices":[{"delta":{"extra_content":{"google":{"thought_signature":"message-fixture"}},"tool_calls":[{"index":0,"id":"g1","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"a.txt\"}"},"extra_content":{"google":{"thought_signature":"tool-fixture"}}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":3}}),
        ] {
            decoder
                .feed(format!("data: {frame}\n\n").as_bytes(), &mut |_| {})
                .unwrap();
        }
        decoder.feed(b"data: [DONE]\n\n", &mut |_| {}).unwrap();
        let response = decoder.finish().unwrap();
        assert_eq!(
            response.message["extra_content"]["google"]["thought_signature"],
            "message-fixture"
        );
        assert_eq!(
            response.message["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
            "tool-fixture"
        );
        assert_eq!(response.usage.output_tokens, Some(3));
    }

    #[test]
    fn native_parameters_and_replay_blocks_are_kept() {
        let c = super::super::profiles::Config::default();
        let (a, m, p) = c.selection("supervisor").unwrap();
        let prior = json!({"role":"assistant","content":[{"type":"thinking","thinking":"summary","signature":"signed"},{"type":"tool_use","id":"t1","name":"read","input":{"path":"x"}}]});
        let body = body(
            &m,
            &a,
            &p.protocol,
            vec![prior.clone()],
            vec![json!({"name":"read"})],
        );
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["messages"][0], prior);
        assert!(body.get("tool_choice").is_none());
        let (mut a, m, p) = c.selection("worker").unwrap();
        a.thinking = false;
        a.effort = None;
        let b = super::body(&m, &a, &p.protocol, vec![], vec![]);
        assert_eq!(b["thinking"]["type"], "disabled");
        assert!(b.get("reasoning_effort").is_none());
    }

    #[test]
    fn anthropic_signed_blocks_tools_and_cumulative_usage_survive_byte_splits() {
        let frames = [
            json!({"type":"message_start","message":{"role":"assistant","content":[],"usage":{"input_tokens":8,"output_tokens":1,"cache_read_input_tokens":5}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"摘要"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed"}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"文档\"}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
            json!({"type":"message_stop"}),
        ];
        let bytes = frames
            .iter()
            .map(|v| format!("data: {v}\r\n\r\n"))
            .collect::<String>();
        let mut d = Decoder::new(Protocol::Anthropic);
        for b in bytes.as_bytes() {
            d.feed(&[*b], &mut |_| {}).unwrap();
        }
        let r = d.finish().unwrap();
        assert_eq!(r.usage.output_tokens, Some(9));
        assert_eq!(r.usage.cache_read_tokens, Some(5));
        assert_eq!(r.message["content"][0]["signature"], "signed");
        assert_eq!(r.message["content"][1]["input"]["path"], "文档");
    }

    #[test]
    fn deepseek_reasoning_tool_fragments_usage_and_early_eof() {
        let mut d = Decoder::new(Protocol::ChatCompletions);
        for v in [
            json!({"choices":[{"delta":{"reasoning_content":"摘要","tool_calls":[{"index":0,"id":"t1","function":{"name":"read","arguments":"{\"path\":"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":11,"completion_tokens":7,"prompt_cache_hit_tokens":3}}),
        ] {
            d.feed(format!("data: {v}\n\n").as_bytes(), &mut |_| {})
                .unwrap();
        }
        d.feed(b"data: [DONE]\n\n", &mut |_| {}).unwrap();
        let r = d.finish().unwrap();
        assert_eq!(r.message["reasoning_content"], "摘要");
        assert_eq!(
            r.message["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"x\"}"
        );
        assert_eq!(r.usage.input_tokens, Some(11));
        assert!(Decoder::new(Protocol::ChatCompletions).finish().is_err());
    }

    #[tokio::test]
    async fn mock_failure_and_cancellation_do_not_succeed() {
        let (tx, mut rx) = watch::channel(false);
        assert!(
            mock_stream(Protocol::Anthropic, "authentication", &mut rx, |_| {})
                .await
                .is_err()
        );
        tx.send(true).unwrap();
        assert!(matches!(
            mock_stream(Protocol::ChatCompletions, "slow", &mut rx, |_| {}).await,
            Err(Failure::Cancelled)
        ));
    }

    #[tokio::test]
    async fn http_stream_works_and_redirect_is_not_followed_or_retried() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for i in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = vec![];
                loop {
                    let mut b = [0; 1024];
                    let n = socket.read(&mut b).await.unwrap();
                    buffer.extend_from_slice(&b[..n]);
                    if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                if i == 0 {
                    socket.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{addr}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                } else {
                    let data="data: {\"choices\":[{\"delta\":{\"content\":\"中文\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()).as_bytes()).await.unwrap();
                    for chunk in data.as_bytes().chunks(7) {
                        socket.write_all(chunk).await.unwrap();
                    }
                }
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        });
        let mut p = super::super::profiles::providers().remove(1);
        p.base_url = format!("http://{addr}");
        let client = client().unwrap();
        let (_tx, mut rx) = watch::channel(false);
        let first = stream(&client, &p, "fake", json!({}), &mut rx, |_| {}).await;
        assert!(matches!(first,Err(Failure::Message(ref s)) if s.contains("307")));
        let result = stream(&client, &p, "fake", json!({}), &mut rx, |_| {})
            .await
            .unwrap();
        assert_eq!(result.text, "中文");
        assert_eq!(result.usage.output_tokens, Some(2));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn stopping_an_open_http_stream_returns_promptly() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 4096];
            let _ = socket.read(&mut b).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            let _ = ready_tx.send(());
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let mut p = super::super::profiles::providers().remove(1);
        p.base_url = format!("http://{addr}");
        let client = client().unwrap();
        let (tx, mut rx) = watch::channel(false);
        let request =
            tokio::spawn(
                async move { stream(&client, &p, "fake", json!({}), &mut rx, |_| {}).await },
            );
        ready_rx.await.unwrap();
        tx.send(true).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(500), request)
                .await
                .unwrap()
                .unwrap(),
            Err(Failure::Cancelled)
        ));
        server.abort();
    }
}
