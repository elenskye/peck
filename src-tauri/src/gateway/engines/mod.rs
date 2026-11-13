//! CLI 引擎（[`AgentBackend`] 的真实现）：起 CLI 进程，一行一行收发 JSON，转成统一事件，
//! 控制通道的审批、回答、追加、打断写回给 CLI。
//!
//! - [`codex::CodexBackend`]：`codex app-server`（JSON-RPC）。
//! - [`claude_code::ClaudeCodeBackend`]：`claude -p` 的双向 stream-json，审批走 stdio。
//! - [`muse::MuseBackend`]：`muse serve`（会话协议 MSP）。
//! - [`acp::AcpBackend`]：Grok（`grok agent stdio`）、Antigravity（`agy acp`），都是 ACP。
//!
//! 进程放进自己的进程组，收尾时先关 stdin、等它退出，超时再结束整组。
//! 测试用 [`Pipe::script`]：按录下的原始输出回放，碰到「该 Peck 回话」的地方停下等 Peck 回。

pub mod acp;
pub mod claude_code;
pub mod codex;
pub mod muse;

use super::config::GatewayConfig;
use super::convert::Emitted;
use super::event::EventBody;
use super::log::EventSink;
use super::{AgentBackend, ErrorKind, GatewayError};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;

/// 起一个 CLI 要的东西。
#[derive(Debug, Clone)]
pub struct Launch {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// 额外的环境变量（账号目录 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` 这类）。
    pub env: Vec<(String, String)>,
    /// 子进程的 PATH（带上「CLI 目录」）。
    pub path: String,
}

/// 引擎从哪找、用哪个账号目录。
#[derive(Debug, Clone)]
pub struct EngineSetup {
    pub bin: PathBuf,
    pub path: String,
    pub env: Vec<(String, String)>,
}

/// 在「CLI 目录」、站主 shell 的 PATH、常见的安装位置里找。
pub async fn locate(command: &str, config: &GatewayConfig) -> Option<EngineSetup> {
    let path = config.search_path();
    let bin = crate::agent::resolve_command_path(command, &path).await?;
    Some(EngineSetup {
        bin,
        path,
        env: Vec::new(),
    })
}

/// 发任务时用：引擎 id → 能跑的引擎。模拟模式回放夹具。
pub async fn backend_for(
    engine: &str,
    config: &GatewayConfig,
    mock: bool,
    account_env: Vec<(String, String)>,
) -> Result<Arc<dyn AgentBackend>, GatewayError> {
    if mock {
        return super::mock::ReplayBackend::fixture(engine, Duration::from_millis(250))
            .map(|b| Arc::new(b) as Arc<dyn AgentBackend>)
            .ok_or_else(|| GatewayError::new(ErrorKind::Engine, format!("{engine} 还没有模拟数据。")));
    }
    let spec = super::catalog::catalog()
        .engines
        .iter()
        .find(|e| e.id == engine)
        .ok_or_else(|| GatewayError::new(ErrorKind::BadRequest, format!("不认识的引擎 {engine}。")))?;
    let mut setup = locate(&spec.command, config).await.ok_or_else(|| {
        GatewayError::new(
            ErrorKind::Engine,
            format!("没找到 {}：装好以后在设置的「CLI 检测」里填 CLI 目录。", spec.label),
        )
    })?;
    setup.env = account_env;
    match engine {
        "codex" => Ok(Arc::new(codex::CodexBackend::new(setup))),
        "claude-code" => Ok(Arc::new(claude_code::ClaudeCodeBackend::new(setup))),
        "muse" => Ok(Arc::new(muse::MuseBackend::new(setup))),
        "grok" => Ok(Arc::new(acp::AcpBackend::new(setup, acp::GROK))),
        "antigravity" => Ok(Arc::new(acp::AcpBackend::new(setup, acp::ANTIGRAVITY))),
        _ => Err(GatewayError::new(ErrorKind::BadRequest, format!("不认识的引擎 {engine}。"))),
    }
}

/// 发任务时用：按 App 的设置拿引擎（CLI 目录、模拟模式）。账号就是客户端那一个（CLI 的默认位置），不加环境变量。
pub async fn from_state(state: &crate::state::AppState, engine: &str) -> Result<Arc<dyn AgentBackend>, GatewayError> {
    let internal = || GatewayError::new(ErrorKind::Internal, "读不到设置。");
    let mock = *state.api.mode.lock().map_err(|_| internal())? == crate::mode::ApiMode::Mock;
    let config = {
        let store = state.store.lock().map_err(|_| internal())?;
        GatewayConfig::load(&store).map_err(|_| internal())?
    };
    backend_for(engine, &config, mock, Vec::new()).await
}

/// CLI 输出的一行：JSON，或者读不懂的文字。
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Json(Value),
    Text(String),
}

/// 和 CLI 之间的一对管道：真进程，或测试用的回放。
pub enum Pipe {
    Process(ProcessPipe),
    Script(ScriptPipe),
}

impl Pipe {
    pub fn spawn(launch: &Launch) -> Result<Self, GatewayError> {
        ProcessPipe::spawn(launch).map(Self::Process)
    }

    /// 回放一份录下的原始输出（`raw.jsonl`）。
    pub fn script(raw_jsonl: &str) -> Self {
        Self::Script(ScriptPipe::new(raw_jsonl))
    }

    pub async fn send(&mut self, value: &Value) -> Result<(), GatewayError> {
        match self {
            Self::Process(p) => p.send(value).await,
            Self::Script(s) => {
                s.sent.push(value.clone());
                Ok(())
            }
        }
    }

    /// 下一行；进程退出（或回放完）为 `None`。可以放进 `select!`：中途取消不丢行。
    pub async fn next(&mut self) -> Option<Line> {
        match self {
            Self::Process(p) => p.lines.recv().await,
            Self::Script(s) => s.next().await,
        }
    }

    pub fn close_stdin(&mut self) {
        if let Self::Process(p) = self {
            p.stdin = None;
        }
    }

    /// 给进程组发信号（打断用；没有进程组就什么都不做）。
    pub async fn signal(&self, signal: &str) {
        if let Self::Process(p) = self {
            if let Some(pgid) = p.pgid {
                let _ = tokio::process::Command::new("/bin/kill")
                    .args([format!("-{signal}"), "--".into(), format!("-{pgid}")])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
            }
        }
    }

    /// 收尾：关 stdin，等 `grace`，还没退就结束整个进程组。
    pub async fn stop(mut self, grace: Duration) {
        self.close_stdin();
        if let Self::Process(mut p) = self {
            if tokio::time::timeout(grace, p.child.wait()).await.is_err() {
                crate::process::terminate_process_group(p.pgid).await;
                let _ = p.child.kill().await;
            }
        }
    }

    /// stderr 最后几行（出错时给站主看）。
    pub fn stderr_tail(&self) -> String {
        match self {
            Self::Process(p) => p
                .stderr
                .lock()
                .map(|l| l.iter().cloned().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default(),
            Self::Script(_) => String::new(),
        }
    }

    /// 回放时 Peck 发出去的（测试看）。
    pub fn sent(&self) -> &[Value] {
        match self {
            Self::Script(s) => &s.sent,
            Self::Process(_) => &[],
        }
    }
}

pub struct ProcessPipe {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::UnboundedReceiver<Line>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    pgid: Option<u32>,
}

impl ProcessPipe {
    fn spawn(launch: &Launch) -> Result<Self, GatewayError> {
        let mut cmd = tokio::process::Command::new(&launch.bin);
        cmd.args(&launch.args)
            .current_dir(&launch.cwd)
            .env("PATH", &launch.path)
            .envs(launch.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let name = launch
            .bin
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut child = cmd
            .spawn()
            .map_err(|_| GatewayError::new(ErrorKind::Engine, format!("{name} 起不来：看看设置的「CLI 检测」。")))?;
        let pgid = child.id();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let (tx, lines) = mpsc::unbounded_channel();
        if let Some(stdout) = stdout {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let parsed = match serde_json::from_str::<Value>(&line) {
                        Ok(v) => Line::Json(v),
                        Err(_) => Line::Text(line),
                    };
                    if tx.send(parsed).is_err() {
                        break;
                    }
                }
            });
        }
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(pipe) = stderr_pipe {
            let tail = stderr.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Ok(mut t) = tail.lock() {
                        t.push_back(super::convert::short(&line, 300));
                        if t.len() > 12 {
                            t.pop_front();
                        }
                    }
                }
            });
        }
        Ok(Self {
            child,
            stdin,
            lines,
            stderr,
            pgid,
        })
    }

    async fn send(&mut self, value: &Value) -> Result<(), GatewayError> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(GatewayError::new(ErrorKind::Engine, "引擎的输入已经关了。"));
        };
        let mut line = serde_json::to_vec(value).unwrap_or_default();
        line.push(b'\n');
        stdin
            .write_all(&line)
            .await
            .map_err(|_| GatewayError::new(ErrorKind::Engine, "写不进引擎（它可能已经退出）。"))?;
        stdin
            .flush()
            .await
            .map_err(|_| GatewayError::new(ErrorKind::Engine, "写不进引擎（它可能已经退出）。"))
    }
}

/// 回放录下的原始输出。三条规矩让它和真 CLI 一样「等 Peck」：
/// 1. Peck 发出第一行之前什么都不出（CLI 等输入）；
/// 2. JSON-RPC 的回应（有 `id`、没 `method`）要等 Peck 先发了同 `id` 的请求；
/// 3. CLI 发来的请求（JSON-RPC 带 `method` 的、Claude Code 的 `control_request`）出了以后，
///    要等 Peck 回了才接着出。
pub struct ScriptPipe {
    lines: VecDeque<Value>,
    sent: Vec<Value>,
    waiting_reply: Option<Reply>,
}

#[derive(Debug, Clone)]
enum Reply {
    Rpc(Value),
    Control(String),
}

impl ScriptPipe {
    fn new(raw_jsonl: &str) -> Self {
        Self {
            lines: raw_jsonl
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            sent: Vec::new(),
            waiting_reply: None,
        }
    }

    fn replied(&self, reply: &Reply) -> bool {
        self.sent.iter().any(|s| match reply {
            Reply::Rpc(id) => s.get("method").is_none() && s.get("id") == Some(id),
            Reply::Control(id) => {
                s["type"] == "control_response" && s["response"]["request_id"].as_str() == Some(id.as_str())
            }
        })
    }

    fn ready(&self) -> bool {
        if self.sent.is_empty() {
            return false;
        }
        if let Some(reply) = &self.waiting_reply {
            if !self.replied(reply) {
                return false;
            }
        }
        match self.lines.front() {
            Some(line) if is_rpc_response(line) => {
                let id = line.get("id");
                self.sent
                    .iter()
                    .any(|s| s.get("method").is_some() && s.get("id") == id)
            }
            _ => true,
        }
    }

    async fn next(&mut self) -> Option<Line> {
        if self.lines.is_empty() {
            return None;
        }
        if !self.ready() {
            // 等 Peck 回话：在 `select!` 里会被控制通道那一支取消，下一轮重新看。
            std::future::pending::<()>().await;
        }
        self.waiting_reply = None;
        let line = self.lines.pop_front()?;
        if let (Some(_), Some(id)) = (line.get("method"), line.get("id")) {
            self.waiting_reply = Some(Reply::Rpc(id.clone()));
        } else if line["type"] == "control_request" {
            if let Some(id) = line["request_id"].as_str() {
                self.waiting_reply = Some(Reply::Control(id.into()));
            }
        }
        Some(Line::Json(line))
    }
}

/// JSON-RPC 的回应：有 `id`、有 `result` 或 `error`、没有 `method`（Codex 不带 `jsonrpc` 字段）。
pub(crate) fn is_rpc_response(line: &Value) -> bool {
    line.get("method").is_none()
        && line.get("id").is_some()
        && (line.get("result").is_some() || line.get("error").is_some())
}

/// 转换器出的事件发出去：原件挂在这一行的第一条上，子任务的走子任务的 sink。返回发了什么。
pub(crate) fn emit_all(sink: &EventSink, emitted: Vec<Emitted>, raw: Option<&Value>) -> Vec<EventBody> {
    let mut out = Vec::with_capacity(emitted.len());
    for (i, e) in emitted.into_iter().enumerate() {
        let raw = if i == 0 { raw.cloned() } else { None };
        match &e.subagent_id {
            Some(id) => sink.for_subagent(id.clone()).emit_raw(e.body.clone(), raw),
            None => sink.emit_raw(e.body.clone(), raw),
        };
        out.push(e.body);
    }
    out
}

/// 引擎中途没了：补一条错误和一条回合结束（统一事件最后一条总是 `turn.completed`）。
pub(crate) fn emit_engine_died(sink: &EventSink, label: &str, turn_id: &str, stderr: &str) -> String {
    let tail = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    let message = if tail.is_empty() {
        format!("{label} 中途退出了。")
    } else {
        format!("{label} 中途退出了：{tail}")
    };
    sink.emit(EventBody::Error {
        message: message.clone(),
        fatal: true,
    });
    sink.emit(EventBody::TurnCompleted {
        turn_id: turn_id.into(),
        reason: super::event::TurnEndReason::Error,
        error: Some(message.clone()),
    });
    message
}

#[cfg(test)]
pub(crate) mod testing {
    //! 引擎测试用：一个会自动回答审批和提问的 sink。
    use super::*;
    use crate::gateway::event::{AgentEvent, ApprovalDecision};
    use crate::gateway::Control;

    /// 每来一个审批就按 `decide` 回答；提问一律回答 `answer`。返回 sink 和收到的事件。
    pub fn auto_answer(
        engine: &str,
        control: mpsc::UnboundedSender<Control>,
        decide: impl Fn(&str) -> ApprovalDecision + Send + Sync + 'static,
        answer: &'static str,
    ) -> (EventSink, Arc<Mutex<Vec<AgentEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        let sink = EventSink::new("test-run", None, engine, None, move |e: &AgentEvent| {
            seen.lock().unwrap().push(e.clone());
            match &e.body {
                EventBody::ApprovalRequested { request_id, title, .. } => {
                    let _ = control.send(Control::Approve {
                        request_id: request_id.clone(),
                        decision: decide(title),
                    });
                }
                EventBody::UserInputRequested { request_id, .. } => {
                    let _ = control.send(Control::Answer {
                        request_id: request_id.clone(),
                        answer: answer.into(),
                    });
                }
                _ => {}
            }
        });
        (sink, events)
    }
}
