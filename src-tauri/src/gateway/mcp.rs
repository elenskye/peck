//! Peck 给 CLI 的本机 MCP 入口：只听 `127.0.0.1`，streamable HTTP（只回 JSON，不开 SSE），
//! 每个任务一个随机令牌（`Authorization: Bearer …`）。工具只有一个 `ask_user`：引擎问站主一句，
//! Peck 弹 ASK 卡片，站主答了把答案还给引擎。
//!
//! 谁用：Grok、Antigravity（ACP 自己没有「问站主」，建会话时把这个入口放进 `mcpServers`）。
//! Codex（`requestUserInput`）、Claude Code（`AskUserQuestion`）、Muse（`userInput/request`）
//! 有自己的通道，不注入。
//!
//! 第一次用时起服务（随机端口），App 开着就一直在。令牌只在内存里，不进事件、不进日志。

use super::event::EventBody;
use super::log::EventSink;
use super::{ErrorKind, GatewayError, PeckMcp};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

const BODY_LIMIT: usize = 1024 * 1024;
const PROTOCOL: &str = "2025-06-18";

pub struct McpHub {
    addr: SocketAddr,
    tasks: Mutex<HashMap<String, EventSink>>,
    /// 等站主回答的提问：request id → (令牌, 回答的通道)。
    waiting: Mutex<HashMap<String, (String, oneshot::Sender<String>)>>,
    counter: AtomicU64,
}

/// 整个 App 共用一个。第一次调用时起服务。
pub async fn hub() -> Result<Arc<McpHub>, GatewayError> {
    static HUB: tokio::sync::OnceCell<Arc<McpHub>> = tokio::sync::OnceCell::const_new();
    HUB.get_or_try_init(McpHub::start).await.cloned()
}

impl McpHub {
    pub async fn start() -> Result<Arc<Self>, GatewayError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| GatewayError::new(ErrorKind::Internal, "Peck 的本机 MCP 起不来。"))?;
        let addr = listener
            .local_addr()
            .map_err(|_| GatewayError::new(ErrorKind::Internal, "Peck 的本机 MCP 起不来。"))?;
        let hub = Arc::new(Self {
            addr,
            tasks: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        });
        let serving = hub.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let hub = serving.clone();
                tokio::spawn(async move {
                    let _ = hub.serve(socket).await;
                });
            }
        });
        Ok(hub)
    }

    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }

    /// 给一个任务开入口：问题经 `sink` 发成 ASK 卡片。任务结束要 [`McpHub::unregister`]。
    pub fn register(&self, sink: EventSink) -> PeckMcp {
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        if let Ok(mut t) = self.tasks.lock() {
            t.insert(token.clone(), sink);
        }
        PeckMcp { url: self.url(), token }
    }

    /// 任务结束：令牌作废，还在等的提问都回「任务已经结束」。
    pub fn unregister(&self, token: &str) {
        if let Ok(mut t) = self.tasks.lock() {
            t.remove(token);
        }
        if let Ok(mut w) = self.waiting.lock() {
            w.retain(|_, (owner, _)| owner != token);
        }
    }

    /// 站主回答了：交给在等的那次 `ask_user`。不是这里的提问返回 `false`。
    pub fn answer(&self, request_id: &str, answer: &str) -> bool {
        let waiting = self.waiting.lock().ok().and_then(|mut w| w.remove(request_id));
        match waiting {
            Some((_, tx)) => tx.send(answer.to_string()).is_ok(),
            None => false,
        }
    }

    async fn serve(&self, mut socket: tokio::net::TcpStream) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let head_end = loop {
            let mut chunk = [0u8; 4096];
            let n = socket.read(&mut chunk).await?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i;
            }
            if buf.len() > 64 * 1024 {
                return respond(&mut socket, 431, "", None).await;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or("");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let headers: HashMap<String, String> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        if length > BODY_LIMIT {
            return respond(&mut socket, 413, "", None).await;
        }
        let mut body = buf[head_end + 4..].to_vec();
        while body.len() < length {
            let mut chunk = vec![0u8; length - body.len()];
            let n = socket.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        if path != "/mcp" {
            return respond(&mut socket, 404, "", None).await;
        }
        let token = headers
            .get("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("")
            .to_string();
        let sink = self.tasks.lock().ok().and_then(|t| t.get(&token).cloned());
        let Some(sink) = sink else {
            return respond(&mut socket, 401, "", None).await;
        };
        match method {
            "POST" => {}
            "DELETE" => return respond(&mut socket, 200, "", None).await,
            _ => return respond(&mut socket, 405, "", None).await,
        }
        let Ok(message) = serde_json::from_slice::<Value>(&body) else {
            let err = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}});
            return respond(&mut socket, 400, &err.to_string(), None).await;
        };
        let session = headers.get("mcp-session-id").cloned();
        let (reply, new_session) = match &message {
            Value::Array(batch) => {
                let mut out = Vec::new();
                for m in batch {
                    if let Some(r) = self.handle(m, &token, &sink).await {
                        out.push(r);
                    }
                }
                ((!out.is_empty()).then_some(Value::Array(out)), None)
            }
            m => {
                let fresh = (m["method"] == "initialize").then(|| uuid::Uuid::new_v4().to_string());
                (self.handle(m, &token, &sink).await, fresh)
            }
        };
        let session_header = new_session.or(session);
        match reply {
            Some(r) => respond(&mut socket, 200, &r.to_string(), session_header.as_deref()).await,
            None => respond(&mut socket, 202, "", session_header.as_deref()).await,
        }
    }

    /// 一条 JSON-RPC。通知（没有 `id`）返回 `None`。
    async fn handle(&self, m: &Value, token: &str, sink: &EventSink) -> Option<Value> {
        let id = m.get("id").cloned()?;
        let ok = |result: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": result}));
        match m["method"].as_str().unwrap_or("") {
            "initialize" => ok(json!({
                "protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "ask_user asks the person you are working for and waits for their answer."
            })),
            "ping" => ok(json!({})),
            "tools/list" => ok(json!({"tools": [{
                "name": "ask_user",
                "title": "Ask the user",
                "description": "Ask the person you are working for one question and wait for their answer. Use it when you need a decision or information only they have. Offer 2-4 short options when the answer is a choice.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "question": {"type": "string", "description": "The question, in the user's language."},
                        "options": {"type": "array", "items": {"type": "string"}, "description": "Optional short choices."}
                    },
                    "required": ["question"]
                }
            }]})),
            "tools/call" if m["params"]["name"] == "ask_user" => {
                let args = &m["params"]["arguments"];
                let question = args["question"].as_str().unwrap_or("").trim().to_string();
                if question.is_empty() {
                    return ok(json!({"content": [{"type": "text", "text": "question is required"}], "isError": true}));
                }
                let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
                let request_id = format!("peck-ask-{n}");
                let (tx, rx) = oneshot::channel();
                if let Ok(mut w) = self.waiting.lock() {
                    w.insert(request_id.clone(), (token.to_string(), tx));
                }
                let options: Vec<String> = args["options"]
                    .as_array()
                    .map(|o| o.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                sink.emit(EventBody::UserInputRequested {
                    request_id,
                    question,
                    options,
                    free_text: true,
                });
                match rx.await {
                    Ok(answer) => ok(json!({"content": [{"type": "text", "text": answer}]})),
                    Err(_) => ok(json!({"content": [{"type": "text", "text": "The task ended before the user answered."}], "isError": true})),
                }
            }
            "tools/call" => ok(json!({"content": [{"type": "text", "text": "unknown tool"}], "isError": true})),
            "resources/list" => ok(json!({"resources": []})),
            "prompts/list" => ok(json!({"prompts": []})),
            other => Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {other}")}})),
        }
    }
}

async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
    session: Option<&str>,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if let Some(s) = session {
        head.push_str(&format!("Mcp-Session-Id: {s}\r\n"));
    }
    head.push_str("\r\n");
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body.as_bytes()).await?;
    socket.shutdown().await
}

/// 引擎的控制通道收到一个回答：是 Peck 的 `ask_user` 的就交过去，并发 `user_input.resolved`。
pub(crate) async fn try_answer(request_id: &str, answer: &str, sink: &EventSink) -> bool {
    if !request_id.starts_with("peck-ask-") {
        return false;
    }
    let delivered = match hub().await {
        Ok(h) => h.answer(request_id, answer),
        Err(_) => false,
    };
    if delivered {
        sink.emit(EventBody::UserInputResolved {
            request_id: request_id.into(),
            answer: answer.into(),
        });
    }
    delivered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::AgentEvent;

    async fn post(url: &str, token: &str, body: Value) -> (u16, Value) {
        let client = reqwest::Client::new();
        let r = client
            .post(url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn ask_user_waits_for_the_answer_and_tokens_are_checked() {
        let hub = McpHub::start().await.unwrap();
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
        let s2 = seen.clone();
        let sink = EventSink::new("r", None, "grok", None, move |e: &AgentEvent| s2.lock().unwrap().push(e.clone()));
        let mcp = hub.register(sink);
        assert!(mcp.url.starts_with("http://127.0.0.1:"));

        let (status, _) = post(&mcp.url, "wrong", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
        assert_eq!(status, 401, "令牌不对就拒绝");

        let (_, init) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).await;
        assert_eq!(init["result"]["serverInfo"]["name"], "peck");
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        assert_eq!(tools["result"]["tools"][0]["name"], "ask_user");

        let url = mcp.url.clone();
        let token = mcp.token.clone();
        let call = tokio::spawn(async move {
            post(&url, &token, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "ask_user", "arguments": {"question": "用哪个颜色？", "options": ["蓝", "红"]}}})).await
        });
        // 等 ASK 卡片出来再回答。
        let request_id = loop {
            if let Some(id) = seen.lock().unwrap().iter().find_map(|e| match &e.body {
                EventBody::UserInputRequested { request_id, options, .. } if options.len() == 2 => Some(request_id.clone()),
                _ => None,
            }) {
                break id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert!(hub.answer(&request_id, "蓝"));
        let (_, result) = call.await.unwrap();
        assert_eq!(result["result"]["content"][0]["text"], "蓝");

        // 任务结束：还在等的提问回「结束了」，令牌作废。
        let url = mcp.url.clone();
        let token = mcp.token.clone();
        let pending = tokio::spawn(async move {
            post(&url, &token, json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "ask_user", "arguments": {"question": "还在吗"}}})).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        hub.unregister(&mcp.token);
        let (_, result) = pending.await.unwrap();
        assert_eq!(result["result"]["isError"], true);
        let (status, _) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 5, "method": "ping"})).await;
        assert_eq!(status, 401);
    }
}
