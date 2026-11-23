//! Peck 给 CLI 的本机 MCP 入口：只听 `127.0.0.1`，streamable HTTP（只回 JSON，不开 SSE），
//! 每个任务一个随机令牌（`Authorization: Bearer …`）。工具按这个任务给的（[`Offer`]）：
//! - `ask_user`：引擎问站主一句，Peck 弹 ASK 卡片，站主答了把答案还给引擎。只给 Grok、Antigravity（ACP 自己没有
//!   「问站主」）；Codex（`requestUserInput`）、Claude Code（`AskUserQuestion`）、Muse（`userInput/request`）有自己的通道。
//! - `delegate_task`：选了委派时（v0.9 B1，见 `harness/delegate.rs`）。
//! - `create_skill`：提议存一条 Skill（v0.10 C3，见 `skill_propose`），五家 CLI 都给（站主 2026-11-16 定）。
//! - `restore_snapshot`、`schedule_task`、`list_schedules`、`cancel_schedule`（v1.0.2，见 `harness/extras.rs`）：
//!   快照回退和定时任务，和 Peck Loop 同一套；回退先弹高危卡片，站主批了才动。
//!
//! - `load_skill`（v1.1.0）：读自带插件的 Skill 全文（怎么用上面这几样工具）。自带插件不写进项目，CLI 自己读不到，
//!   所以这一次给了哪个自带插件的工具，就能读它的 Skill。只读。
//! - `git_commit`（v1.1.0，见 `harness/git.rs`）：项目里装着要它的插件（Git Plan）、这一段不是只读的才给。EDIT 档
//!   先弹一张卡片（能「总是允许」），AUTO 不问。
//!
//! 所以 v0.10 起每个 CLI 任务都挂这个入口（只这一次进程，不改 CLI 的配置）。
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

/// 一个任务的入口给哪些工具。
#[derive(Clone, Default)]
pub struct Offer {
    /// 给 `ask_user`（引擎自己没有问站主的通道时）。
    pub ask_user: bool,
    /// 给 `delegate_task`（v0.9 B1）。
    pub delegate: Option<Arc<crate::harness::delegate::CliDelegate>>,
    /// 给 `create_skill`（v0.10 C3）：项目根，提议的 Skill 写进它的 `.claude/skills`。
    pub skill_root: Option<std::path::PathBuf>,
    /// 给快照回退和定时任务那四个（v1.0.2）。
    pub extras: Option<Arc<crate::harness::extras::Extras>>,
    /// 这一段只读（PLAN、合作里写规范和审查的）：不回退文件。
    pub read_only: bool,
    /// 这个项目里不给的 Peck 工具（v1.1.0）：项目设置里关掉的自带插件的那几个。
    pub off: Vec<String>,
    /// 给 `git_commit`（v1.1.0）：在哪个文件夹里提交；`ask` 是 EDIT 档（没「总是允许」过就先问站主）。
    /// 要有 `extras`（卡片、「总是允许」靠它里面的任务号和项目号）。
    pub git: Option<GitOffer>,
}

#[derive(Clone)]
pub struct GitOffer {
    pub root: std::path::PathBuf,
    pub ask: bool,
}

impl GitOffer {
    /// 这个项目装着要 `git_commit` 的插件、这一段不是 PLAN 才给。
    pub fn of(project: &std::path::Path, cwd: &std::path::Path, level: super::event::PermissionLevel) -> Option<Self> {
        use super::event::PermissionLevel;
        let wanted = crate::project_plugins::tools_on(project).iter().any(|t| t == crate::harness::git::GIT_COMMIT);
        (wanted && level != PermissionLevel::Plan).then(|| Self { root: cwd.to_path_buf(), ask: level == PermissionLevel::Edit })
    }
}

impl Offer {
    /// 这个工具给不给（没被项目关掉）。
    fn has(&self, tool: &str) -> bool {
        !self.off.iter().any(|t| t == tool)
    }

    /// 给 `git_commit`。
    fn git(&self) -> Option<(&GitOffer, &Arc<crate::harness::extras::Extras>)> {
        self.git.as_ref().zip(self.extras.as_ref()).filter(|_| !self.read_only)
    }

    /// 这一次能读的自带 Skill：给了哪个自带插件的工具，就带上它的 Skill。
    fn builtin_skills(&self) -> Vec<&'static crate::plugins::SkillText> {
        let mut offered = self.extra_names();
        if self.skill().is_some() {
            offered.push("create_skill");
        }
        crate::plugins::builtin().iter().filter(|p| p.info.tools.iter().any(|t| offered.contains(&t.name.as_str()))).flat_map(|p| p.skills.iter()).collect()
    }

    /// 给 `create_skill`。
    fn skill(&self) -> Option<&std::path::PathBuf> {
        self.skill_root.as_ref().filter(|_| self.has("create_skill"))
    }

    /// 快照回退、定时任务那四个里现在给的。
    fn extra_names(&self) -> Vec<&'static str> {
        if self.extras.is_none() {
            return vec![];
        }
        crate::harness::extras::NAMES.into_iter().filter(|n| self.has(n)).collect()
    }
}

/// 一个任务登记的入口：事件往哪发、给哪些工具。
#[derive(Clone)]
struct Registered {
    sink: EventSink,
    offer: Offer,
}

pub struct McpHub {
    addr: SocketAddr,
    tasks: Mutex<HashMap<String, Registered>>,
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

    /// 给一个任务开入口，只有 `ask_user`：问题经 `sink` 发成 ASK 卡片。任务结束要 [`McpHub::unregister`]。
    #[cfg(test)]
    pub fn register(&self, sink: EventSink) -> PeckMcp {
        self.register_offer(sink, Offer { ask_user: true, ..Default::default() })
    }

    /// 给一个任务开入口，工具照 `offer` 给。
    pub fn register_offer(&self, sink: EventSink, offer: Offer) -> PeckMcp {
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        if let Ok(mut t) = self.tasks.lock() {
            t.insert(token.clone(), Registered { sink, offer });
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
        let entry = self.tasks.lock().ok().and_then(|t| t.get(&token).cloned());
        let Some(entry) = entry else {
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
                    if let Some(r) = self.handle(m, &token, &entry).await {
                        out.push(r);
                    }
                }
                ((!out.is_empty()).then_some(Value::Array(out)), None)
            }
            m => {
                let fresh = (m["method"] == "initialize").then(|| uuid::Uuid::new_v4().to_string());
                (self.handle(m, &token, &entry).await, fresh)
            }
        };
        let session_header = new_session.or(session);
        match reply {
            Some(r) => respond(&mut socket, 200, &r.to_string(), session_header.as_deref()).await,
            None => respond(&mut socket, 202, "", session_header.as_deref()).await,
        }
    }

    /// 一条 JSON-RPC。通知（没有 `id`）返回 `None`。
    async fn handle(&self, m: &Value, token: &str, entry: &Registered) -> Option<Value> {
        let sink = &entry.sink;
        let id = m.get("id").cloned()?;
        let ok = |result: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": result}));
        let offer = &entry.offer;
        match m["method"].as_str().unwrap_or("") {
            "initialize" => {
                let mut lines: Vec<&str> = Vec::new();
                if offer.ask_user {
                    lines.push("ask_user asks the person you are working for and waits for their answer.");
                }
                if offer.delegate.is_some() {
                    lines.push("delegate_task hands a self-contained subtask to a cheaper model working in the same project and returns its report.");
                }
                if offer.skill().is_some() {
                    lines.push("create_skill proposes saving a reusable procedure you worked out as a skill (SKILL.md); the user approves it. Use it instead of writing skill files yourself.");
                }
                if offer.git().is_some() {
                    lines.push("git_commit commits changes in this project's repository (commit only: no push, no history rewriting, no signature). Use it instead of running git commit yourself; Peck already commits each finished round on its own, so call it only for an extra commit or when the user asks.");
                }
                let extra = offer.extra_names();
                if extra.contains(&crate::harness::extras::RESTORE_SNAPSHOT) {
                    lines.push("restore_snapshot goes back to Peck's snapshot of the project from before or after an earlier task (undo / redo what a task changed); use it when the user asks to undo, revert or restore a task's changes, never on your own, and restate what you are about to do and get the user's yes first.");
                }
                if extra.contains(&crate::harness::extras::SCHEDULE_TASK) {
                    lines.push("schedule_task, list_schedules and cancel_schedule manage scheduled tasks in this project; use them when the user asks for something to happen at a time or on a schedule, and restate the schedule and get the user's yes first.");
                }
                let skills = offer.builtin_skills();
                let mut lines: Vec<String> = lines.into_iter().map(str::to_string).collect();
                if !skills.is_empty() {
                    let names = skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
                    lines.push(format!("Before using these tools, read how with load_skill (name: {names}) and follow it."));
                }
                ok(json!({
                    "protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": lines.join(" ")
                }))
            }
            "ping" => ok(json!({})),
            "tools/list" => {
                let mut tools = Vec::new();
                if offer.ask_user {
                    tools.push(json!({
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
                    },
                    "annotations": {"readOnlyHint": true}
                    }));
                }
                if let Some(d) = &offer.delegate {
                    tools.push(json!({
                        "name": "delegate_task",
                        "title": "Delegate a subtask",
                        "description": crate::harness::delegate::tool_description(d),
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "task": {"type": "string", "description": "The subtask, complete enough to do without asking: what to change, where, how to check it."},
                                "files": {"type": "array", "items": {"type": "string"}, "description": "Optional project-relative paths to start with."}
                            },
                            "required": ["task"]
                        },
                        "annotations": {"readOnlyHint": false, "destructiveHint": false}
                    }));
                }
                if offer.skill().is_some() {
                    tools.push(json!({
                        "name": "create_skill",
                        "title": "Propose a skill",
                        "description": crate::skill_propose::TOOL_DESCRIPTION,
                        "inputSchema": crate::skill_propose::tool_schema(),
                        // 只是提议，不写盘（站主批了才写）。
                        "annotations": {"readOnlyHint": true}
                    }));
                }
                let skills = offer.builtin_skills();
                if !skills.is_empty() {
                    let list = skills.iter().map(|s| format!("{}: {}", s.name, s.description)).collect::<Vec<_>>().join("\n");
                    tools.push(json!({
                        "name": "load_skill",
                        "title": "Read a Peck skill",
                        "description": format!("Read the full instructions of one of Peck's built-in skills (how to use Peck's own tools). Call it before using the tool the skill covers.\n{list}"),
                        "inputSchema": {"type": "object", "properties": {"name": {"type": "string", "enum": skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()}}, "required": ["name"]},
                        "annotations": {"readOnlyHint": true}
                    }));
                }
                if offer.git().is_some() {
                    use crate::harness::git as g;
                    tools.push(json!({"name": g::GIT_COMMIT, "title": "Commit changes", "description": g::DESCRIPTION, "inputSchema": g::schema(), "annotations": {"readOnlyHint": false, "destructiveHint": false}}));
                }
                {
                    use crate::harness::extras as x;
                    let mut extra = vec![
                        json!({"name": x::RESTORE_SNAPSHOT, "title": "Task snapshots", "description": x::RESTORE_DESCRIPTION, "inputSchema": x::restore_schema(), "annotations": {"readOnlyHint": false, "destructiveHint": true}}),
                        json!({"name": x::SCHEDULE_TASK, "title": "Schedule a task", "description": x::SCHEDULE_DESCRIPTION, "inputSchema": x::schedule_schema(), "annotations": {"readOnlyHint": false, "destructiveHint": false}}),
                        json!({"name": x::LIST_SCHEDULES, "title": "List scheduled tasks", "description": x::LIST_SCHEDULES_DESCRIPTION, "inputSchema": {"type": "object", "properties": {}}, "annotations": {"readOnlyHint": true}}),
                        json!({"name": x::CANCEL_SCHEDULE, "title": "Cancel a scheduled task", "description": x::CANCEL_SCHEDULE_DESCRIPTION, "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}, "annotations": {"readOnlyHint": false, "destructiveHint": false}}),
                    ];
                    // 自带插件按项目开关（v1.1.0）：关掉的不列。
                    let given = offer.extra_names();
                    extra.retain(|t| given.contains(&t["name"].as_str().unwrap_or("")));
                    tools.extend(extra);
                }
                ok(json!({"tools": tools}))
            }
            "tools/call" if offer.extra_names().contains(&m["params"]["name"].as_str().unwrap_or("")) => {
                let Some(x) = offer.extras.clone() else { return None };
                let name = m["params"]["name"].as_str().unwrap_or("").to_string();
                let args = m["params"]["arguments"].clone();
                let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
                let said = extras_call(x, &name, args, sink, offer.read_only, n).await;
                match said {
                    Ok(text) => ok(json!({"content": [{"type": "text", "text": text}]})),
                    Err(why) => ok(json!({"content": [{"type": "text", "text": why}], "isError": true})),
                }
            }
            "tools/call" if m["params"]["name"] == "load_skill" => {
                let name = m["params"]["arguments"]["name"].as_str().unwrap_or("");
                match offer.builtin_skills().into_iter().find(|s| s.name == name) {
                    Some(skill) => ok(json!({"content": [{"type": "text", "text": skill.text}]})),
                    None => ok(json!({"content": [{"type": "text", "text": format!("no built-in skill named {name}")}], "isError": true})),
                }
            }
            "tools/call" if m["params"]["name"] == crate::harness::git::GIT_COMMIT => {
                let Some((g, x)) = offer.git().map(|(g, x)| (g.clone(), x.clone())) else {
                    return ok(json!({"content": [{"type": "text", "text": "git_commit is off for this task"}], "isError": true}));
                };
                let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
                match git_call(g, x, m["params"]["arguments"].clone(), sink, n).await {
                    Ok(text) => ok(json!({"content": [{"type": "text", "text": text}]})),
                    Err(why) => ok(json!({"content": [{"type": "text", "text": why}], "isError": true})),
                }
            }
            "tools/call" if m["params"]["name"] == "create_skill" => {
                let Some(root) = offer.skill().cloned() else {
                    return ok(json!({"content": [{"type": "text", "text": "skills are off for this task"}], "isError": true}));
                };
                let (sink, draft) = (sink.clone(), crate::skill_propose::Draft::from_input(&m["params"]["arguments"]));
                match tokio::task::spawn_blocking(move || crate::skill_propose::propose(&sink, &root, draft)).await {
                    Ok(Ok(text)) => ok(json!({"content": [{"type": "text", "text": text}]})),
                    Ok(Err(why)) => ok(json!({"content": [{"type": "text", "text": why}], "isError": true})),
                    Err(_) => ok(json!({"content": [{"type": "text", "text": "create_skill failed"}], "isError": true})),
                }
            }
            "tools/call" if m["params"]["name"] == "delegate_task" => {
                let Some(d) = &offer.delegate else {
                    return ok(json!({"content": [{"type": "text", "text": "delegation is off for this task"}], "isError": true}));
                };
                let args = &m["params"]["arguments"];
                let files: Vec<String> = args["files"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                match crate::harness::delegate::run(d, sink, args["task"].as_str().unwrap_or(""), &files).await {
                    Ok(text) => ok(json!({"content": [{"type": "text", "text": text}]})),
                    Err(why) => ok(json!({"content": [{"type": "text", "text": why}], "isError": true})),
                }
            }
            "tools/call" if m["params"]["name"] == "ask_user" && offer.ask_user => {
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

/// 快照回退、定时任务那四个工具（CLI 调过来）。回退先算好动哪些文件、弹高危卡片（编号 `peck-tool-<n>-approve`，
/// 站主的回答按这个前缀从任务的控制通道转过来，和委派的子任务同一条路），批了才放回去。
async fn extras_call(x: Arc<crate::harness::extras::Extras>, name: &str, args: Value, sink: &EventSink, read_only: bool, n: u64) -> Result<String, String> {
    use super::event::{ApprovalDecision, ResolvedBy, ToolKind};
    use crate::harness::extras as e;
    let blocking = |f: Box<dyn FnOnce() -> Result<String, String> + Send>| async move { tokio::task::spawn_blocking(f).await.unwrap_or_else(|_| Err("出错了。".into())) };
    if name != e::RESTORE_SNAPSHOT {
        let name = name.to_string();
        return blocking(Box::new(move || match name.as_str() {
            e::SCHEDULE_TASK => x.schedule_task(&args),
            e::LIST_SCHEDULES => x.list_schedules(),
            _ => x.cancel_schedule(&args),
        }))
        .await;
    }
    if args["action"] != "restore" {
        return blocking(Box::new(move || x.list_snapshots())).await;
    }
    if read_only {
        return Err("PLAN 只读：不回退文件。请站主换成 EDIT 或 AUTO 再说一次。".into());
    }
    let planner = x.clone();
    let plan = tokio::task::spawn_blocking(move || planner.plan_restore(&args)).await.map_err(|_| "算要回退哪些文件时出错了。".to_string())??;
    let prefix = format!("peck-tool-{n}-");
    let request_id = format!("{prefix}approve");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    super::runner::register_sub_control(&x.run_id, &prefix, tx);
    let options = vec![ApprovalDecision::AllowOnce, ApprovalDecision::DenyOnce];
    sink.emit(EventBody::ApprovalRequested { request_id: request_id.clone(), call_id: None, kind: ToolKind::Edit, title: plan.title.clone(), detail: Some(plan.detail.clone()), high_risk: true, options });
    // 等站主点；按了停、任务收尾（`interrupt_subs`）、通道断了都算没批。
    let decision = loop {
        match rx.recv().await {
            Some(super::Control::Approve { decision, .. }) => break Some(if decision.allows() { ApprovalDecision::AllowOnce } else { ApprovalDecision::DenyOnce }),
            Some(super::Control::Interrupt) | None => break None,
            Some(_) => {}
        }
    };
    super::runner::unregister_sub_control(&x.run_id, &prefix);
    let Some(decision) = decision else {
        return Err("任务停了，站主没批这次回退，文件都没动。".into());
    };
    sink.emit(EventBody::ApprovalResolved { request_id, decision, by: ResolvedBy::User });
    if !decision.allows() {
        return Err("站主没批这次回退，文件都没动。".into());
    }
    tokio::task::spawn_blocking(move || x.restore(&plan)).await.unwrap_or_else(|_| Err("回退出错了。".into()))
}

/// `git_commit`（CLI 调过来）。EDIT 档、这个项目没「总是允许」过：先弹一张卡片（和回退同一条路，不是高危，
/// 多一个「总是允许」），批了才提交。
async fn git_call(g: GitOffer, x: Arc<crate::harness::extras::Extras>, args: Value, sink: &EventSink, n: u64) -> Result<String, String> {
    use super::event::{ApprovalDecision, ResolvedBy, ToolKind};
    use crate::harness::git;
    let allowed = x.store.lock().ok().and_then(|db| db.harness_allowed(&x.workspace_id).ok()).is_some_and(|rules| rules.contains(git::RULE));
    if g.ask && !allowed {
        let prefix = format!("peck-tool-{n}-");
        let request_id = format!("{prefix}approve");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        super::runner::register_sub_control(&x.run_id, &prefix, tx);
        let (title, detail) = git::card(&args);
        let options = vec![ApprovalDecision::AllowOnce, ApprovalDecision::AllowAlways, ApprovalDecision::DenyOnce];
        sink.emit(EventBody::ApprovalRequested { request_id: request_id.clone(), call_id: None, kind: ToolKind::Other, title, detail: Some(detail), high_risk: false, options: options.clone() });
        let decision = loop {
            match rx.recv().await {
                Some(super::Control::Approve { decision, .. }) => break Some(if options.contains(&decision) { decision } else if decision.allows() { ApprovalDecision::AllowOnce } else { ApprovalDecision::DenyOnce }),
                Some(super::Control::Interrupt) | None => break None,
                Some(_) => {}
            }
        };
        super::runner::unregister_sub_control(&x.run_id, &prefix);
        let Some(decision) = decision else {
            return Err("任务停了，站主没批这次提交，什么都没提交。".into());
        };
        sink.emit(EventBody::ApprovalResolved { request_id, decision, by: ResolvedBy::User });
        if !decision.allows() {
            return Err("站主没批这次提交，什么都没提交。".into());
        }
        if decision == ApprovalDecision::AllowAlways {
            if let Ok(db) = x.store.lock() {
                let _ = db.harness_allow(&x.workspace_id, git::RULE);
            }
        }
    }
    tokio::task::spawn_blocking(move || git::call(&g.root, &args)).await.unwrap_or_else(|_| Err("提交出错了。".into()))
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
        // 等第二个提问真的到了服务这边再结束任务（只睡一会儿的话，全量跑负载高时请求还没到就作废了）。
        let asked_twice = || {
            seen.lock().unwrap().iter().filter(|e| matches!(e.body, EventBody::UserInputRequested { .. })).count() >= 2
        };
        for _ in 0..250 {
            if asked_twice() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(asked_twice(), "第二个提问 5 秒内没到");
        hub.unregister(&mcp.token);
        let (_, result) = pending.await.unwrap();
        assert_eq!(result["result"]["isError"], true);
        let (status, _) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 5, "method": "ping"})).await;
        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn snapshots_and_schedules_reach_a_cli_and_restoring_waits_for_the_user() {
        use crate::harness::extras::testing as x;
        let w = x::world();
        w.p.write("a.txt", "a0\n");
        x::task(&w, 940_001, |p| {
            p.write("a.txt", "a1\n");
        });
        let hub = McpHub::start().await.unwrap();
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
        let s2 = seen.clone();
        let sink = EventSink::new("run-mcp-x", None, "codex", None, move |e: &AgentEvent| s2.lock().unwrap().push(e.clone()));
        let mcp = hub.register_offer(sink, Offer { extras: Some(Arc::new(x::extras(&w, "run-mcp-x"))), ..Default::default() });
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).await;
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, [&["load_skill"][..], &crate::harness::extras::NAMES[..]].concat());
        let call = |id: u64, name: &str, args: Value| json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}});
        let (_, r) = post(&mcp.url, &mcp.token, call(2, "schedule_task", json!({"prompt": "看一下 CI", "repeat": "interval", "every_hours": 2}))).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("每隔 2 小时"), "{r}");
        let (_, r) = post(&mcp.url, &mcp.token, call(3, "list_schedules", json!({}))).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("看一下 CI"));
        let (_, r) = post(&mcp.url, &mcp.token, call(4, "restore_snapshot", json!({"action": "list"}))).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("#940001"));

        // 回退：先弹高危卡片，站主的回答从任务的控制通道按编号转过来；批了才动。
        for (decision, text) in [(crate::gateway::event::ApprovalDecision::DenyOnce, "a1\n"), (crate::gateway::event::ApprovalDecision::AllowOnce, "a0\n")] {
            seen.lock().unwrap().clear();
            let (url, token) = (mcp.url.clone(), mcp.token.clone());
            let body = call(5, "restore_snapshot", json!({"action": "restore", "task": 940_001, "to": "before"}));
            let pending = tokio::spawn(async move { post(&url, &token, body).await });
            let request_id = loop {
                if let Some(id) = seen.lock().unwrap().iter().find_map(|e| match &e.body {
                    EventBody::ApprovalRequested { request_id, high_risk: true, options, .. } if options.len() == 2 => Some(request_id.clone()),
                    _ => None,
                }) {
                    break id;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            };
            assert_eq!(w.p.read("a.txt"), "a1\n", "没批之前不动");
            crate::gateway::runner::route_control("run-mcp-x", crate::gateway::Control::Approve { request_id, decision }).unwrap();
            let (_, r) = pending.await.unwrap();
            assert_eq!(r["result"]["isError"].as_bool().unwrap_or(false), !decision.allows(), "{r}");
            assert_eq!(w.p.read("a.txt"), text);
        }
        // 只读的那一段（PLAN）：不回退。
        let sink = EventSink::new("run-mcp-y", None, "codex", None, |_: &AgentEvent| {});
        let ro = hub.register_offer(sink, Offer { extras: Some(Arc::new(x::extras(&w, "run-mcp-y"))), read_only: true, ..Default::default() });
        let (_, r) = post(&ro.url, &ro.token, call(6, "restore_snapshot", json!({"action": "restore", "task": 940_001, "to": "after"}))).await;
        assert!(r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("PLAN"));
        let _ = std::fs::remove_dir_all(&w.root);
    }

    #[tokio::test]
    async fn create_skill_is_offered_and_only_proposes() {
        let hub = McpHub::start().await.unwrap();
        let root = std::env::temp_dir().join(format!("peck-mcp-skill-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
        let s2 = seen.clone();
        let sink = EventSink::new("run-mcp-skill", None, "codex", None, move |e: &AgentEvent| s2.lock().unwrap().push(e.clone()));
        // Codex 这类有自己问站主通道的：只给 create_skill。
        let mcp = hub.register_offer(sink, Offer { skill_root: Some(root.clone()), ..Default::default() });
        let (_, init) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})).await;
        assert!(init["result"]["instructions"].as_str().unwrap().contains("create_skill"));
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, vec!["create_skill", "load_skill"]);
        // 自带的 Skill 不在项目里，CLI 经这里读全文（v1.1.0）：给了哪个插件的工具，就能读它的。
        assert!(init["result"]["instructions"].as_str().unwrap().contains("load_skill (name: peck-skills)"));
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 20, "method": "tools/call", "params": {"name": "load_skill", "arguments": {"name": "peck-skills"}}})).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("# 写一条 Skill"), "{r}");
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 21, "method": "tools/call", "params": {"name": "load_skill", "arguments": {"name": "peck-snapshots"}}})).await;
        assert_eq!(r["result"]["isError"], true, "没给快照的工具，它的 Skill 也不给");
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "create_skill", "arguments": {"name": "peck-c3-mcp", "description": "d", "body": "1. x"}}})).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("站主批了才写进去"), "{r}");
        assert!(seen.lock().unwrap().iter().any(|e| matches!(&e.body, EventBody::SkillProposed { name, .. } if name == "peck-c3-mcp")));
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "create_skill", "arguments": {"name": "Bad Name!", "description": "d", "body": "x"}}})).await;
        assert_eq!(r["result"]["isError"], true);
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "ask_user", "arguments": {"question": "?"}}})).await;
        assert_eq!(r["result"]["isError"], true, "没给 ask_user 的不收");
        // 模型提不了全局的（v1.1.0：Skill 只存进项目）。
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
            "params": {"name": "create_skill", "arguments": {"name": "peck-c3-global", "description": "d", "body": "1. x", "scope": "global"}}})).await;
        assert!(r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("只能存进这个项目"), "{r}");
        assert!(!root.join(".claude").exists());
        hub.unregister(&mcp.token);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_commit_reaches_a_cli_only_where_a_plugin_asks_and_edit_asks_once() {
        use crate::gateway::event::{ApprovalDecision, PermissionLevel};
        use crate::harness::extras::testing as x;
        use crate::harness::git::testing as g;
        let w = x::world();
        g::init(&w.p.dir);
        // 项目里没装要它的插件：不给。
        assert!(GitOffer::of(&w.p.dir, &w.p.dir, PermissionLevel::Auto).is_none());
        g::install_git_plan(&w.p.dir);
        assert!(GitOffer::of(&w.p.dir, &w.p.dir, PermissionLevel::Plan).is_none(), "PLAN 不给");
        let hub = McpHub::start().await.unwrap();
        let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
        let s2 = seen.clone();
        let sink = EventSink::new("run-mcp-git", None, "codex", None, move |e: &AgentEvent| s2.lock().unwrap().push(e.clone()));
        let call = |id: u64, args: Value| json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": "git_commit", "arguments": args}});

        // AUTO：列得出来，直接提交，只提交点名的文件。
        let auto = Offer { extras: Some(Arc::new(x::extras(&w, "run-mcp-git"))), git: GitOffer::of(&w.p.dir, &w.p.dir, PermissionLevel::Auto), ..Default::default() };
        let mcp = hub.register_offer(sink.clone(), auto);
        let (_, init) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": PROTOCOL}})).await;
        assert!(init["result"]["instructions"].as_str().unwrap().contains("git_commit"));
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "git_commit"));
        w.p.write("a.txt", "a\n");
        w.p.write("b.txt", "b\n");
        let (_, r) = post(&mcp.url, &mcp.token, call(3, json!({"message": "v1.0.1: fix some problems", "paths": ["a.txt"]}))).await;
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Committed"), "{r}");
        assert_eq!(g::files_in_head(&w.p.dir), vec!["a.txt"]);
        let (_, r) = post(&mcp.url, &mcp.token, call(4, json!({"message": "update"}))).await;
        assert!(r["result"]["isError"] == true && r["result"]["content"][0]["text"].as_str().unwrap().contains("格式不对"), "{r}");
        assert!(!seen.lock().unwrap().iter().any(|e| matches!(e.body, EventBody::ApprovalRequested { .. })), "AUTO 不问");
        hub.unregister(&mcp.token);

        // EDIT：先弹卡片（不是高危，能「总是允许」）；不批不提交，「总是允许」以后不再问。
        let edit = Offer { extras: Some(Arc::new(x::extras(&w, "run-mcp-git"))), git: GitOffer::of(&w.p.dir, &w.p.dir, PermissionLevel::Edit), ..Default::default() };
        let mcp = hub.register_offer(sink.clone(), edit);
        for (decision, commits) in [(ApprovalDecision::DenyOnce, 2), (ApprovalDecision::AllowAlways, 3)] {
            seen.lock().unwrap().clear();
            let (url, token) = (mcp.url.clone(), mcp.token.clone());
            let body = call(5, json!({"message": "v1.0.1: minor improvement", "paths": ["b.txt"]}));
            let pending = tokio::spawn(async move { post(&url, &token, body).await });
            let request_id = loop {
                if let Some(id) = seen.lock().unwrap().iter().find_map(|e| match &e.body {
                    EventBody::ApprovalRequested { request_id, high_risk: false, options, title, .. } if options.contains(&ApprovalDecision::AllowAlways) && title.contains("v1.0.1: minor improvement") => Some(request_id.clone()),
                    _ => None,
                }) {
                    break id;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            };
            crate::gateway::runner::route_control("run-mcp-git", crate::gateway::Control::Approve { request_id, decision }).unwrap();
            let (_, r) = pending.await.unwrap();
            assert_eq!(r["result"]["isError"].as_bool().unwrap_or(false), !decision.allows(), "{r}");
            assert_eq!(g::log(&w.p.dir, "%s").len(), commits);
        }
        assert!(w.store.lock().unwrap().harness_allowed("wx").unwrap().contains(crate::harness::git::RULE));
        seen.lock().unwrap().clear();
        w.p.write("c.txt", "c\n");
        let (_, r) = post(&mcp.url, &mcp.token, call(6, json!({"message": "v1.0.2: add c", "paths": ["c.txt"]}))).await;
        assert!(r["result"]["isError"].as_bool() != Some(true), "{r}");
        assert!(!seen.lock().unwrap().iter().any(|e| matches!(e.body, EventBody::ApprovalRequested { .. })), "总是允许过就不再问");
        hub.unregister(&mcp.token);

        // 只读的那一段：就算带着也不给、不收。
        let ro = Offer { extras: Some(Arc::new(x::extras(&w, "run-mcp-git"))), git: Some(GitOffer { root: w.p.dir.clone(), ask: false }), read_only: true, ..Default::default() };
        let mcp = hub.register_offer(sink, ro);
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"})).await;
        assert!(!tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "git_commit"));
        let (_, r) = post(&mcp.url, &mcp.token, call(8, json!({"message": "v1.0.3: x"}))).await;
        assert_eq!(r["result"]["isError"], true);
        hub.unregister(&mcp.token);
        let _ = std::fs::remove_dir_all(&w.root);
    }

    #[tokio::test]
    async fn tools_of_a_builtin_plugin_switched_off_are_not_offered() {
        use crate::harness::extras::testing as x;
        let w = x::world();
        let root = std::env::temp_dir().join(format!("peck-mcp-off-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let hub = McpHub::start().await.unwrap();
        let sink = EventSink::new("run-mcp-off", None, "codex", None, |_| {});
        // 项目设置里关了「定时任务」和「写 Skill」两个自带插件。
        let off = crate::plugins::tools_off(&["peck-schedules".into(), "peck-skills".into()]);
        let offer = Offer { skill_root: Some(root.clone()), extras: Some(Arc::new(x::extras(&w, "run-mcp-off"))), off, ..Default::default() };
        let mcp = hub.register_offer(sink, offer);
        let (_, init) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": PROTOCOL}})).await;
        let said = init["result"]["instructions"].as_str().unwrap();
        assert!(said.contains("restore_snapshot") && !said.contains("schedule_task") && !said.contains("create_skill"), "{said}");
        let (_, tools) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, vec!["load_skill", "restore_snapshot"]);
        assert!(said.contains("load_skill (name: peck-snapshots)"), "{said}");
        let list = tools["result"]["tools"][0]["description"].as_str().unwrap();
        assert!(list.contains("peck-snapshots:") && !list.contains("peck-schedules:") && !list.contains("peck-skills:"), "{list}");
        // 硬调也不收。
        for (name, args) in [("schedule_task", json!({"prompt": "x", "repeat": "daily", "time": "09:00"})), ("list_schedules", json!({})), ("create_skill", json!({"name": "peck-off", "description": "d", "body": "x"}))] {
            let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name, "arguments": args}})).await;
            assert_eq!(r["result"]["isError"], true, "{name}: {r}");
        }
        assert!(w.store.lock().unwrap().schedules(Some("wx")).unwrap().is_empty());
        let (_, r) = post(&mcp.url, &mcp.token, json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "restore_snapshot", "arguments": {"action": "list"}}})).await;
        assert!(r["result"]["isError"].as_bool() != Some(true), "{r}");
        hub.unregister(&mcp.token);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(&w.root);
    }
}
