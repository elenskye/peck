//! 七家 API 的 HTTP 这一层：按「线协议 × 认证和地址」拼请求，读 SSE，交给 `convert` 里那一家的流。
//!
//! | 供应商 | 线协议 | 认证 | 地址 |
//! | --- | --- | --- | --- |
//! | Anthropic | Messages | `x-api-key` | `{base}/messages` |
//! | OpenAI | Responses | Bearer | `{base}/responses` |
//! | DeepSeek、百炼 | Chat | Bearer | `{base}/chat/completions`（百炼按地域换 base） |
//! | Gemini | Gemini | `x-goog-api-key` | `{base}/models/{模型}:streamGenerateContent?alt=sse` |
//! | Vertex 上的 Claude、Gemini | Messages、Gemini | ADC 令牌（Bearer） | 按项目和区域拼（`config.rs`） |
//!
//! 不自动重发，不跟随跳转。Key 和令牌不进日志、不进错误。
//! `PECK_RECORD_DIR` 设了就把每次的请求 body 和原始 SSE 存进那个目录（录夹具用，不含 Key）。

use super::catalog::{catalog, Auth, ModelSpec, ProviderSpec};
use super::config::GatewayConfig;
use super::convert::{anthropic, chat, gemini, responses, WireStream};
use super::event::{Origin, Wire};
use super::{BoxFuture, ErrorKind, GatewayError, ModelEvent, ModelProvider, ModelRequest, ModelResponse};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use zeroize::Zeroizing;

/// Anthropic 直连的 API 版本（2026-11-12 实测）。
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const FRAME_LIMIT: usize = 4 * 1024 * 1024;

/// 怎么认证。
pub enum Credential {
    Key(Zeroizing<String>),
    /// Vertex：gcloud 的 ADC 换来的令牌。
    Adc(Arc<AdcToken>),
}

struct Inner {
    provider: ProviderSpec,
    model: ModelSpec,
    origin: Origin,
    url: String,
    credential: Credential,
    region: Option<String>,
    /// Vertex 计配额的项目（`x-goog-user-project`）。
    quota_project: Option<String>,
    client: reqwest::Client,
}

/// 一个 API 模型。`complete` 每次发一个请求。
#[derive(Clone)]
pub struct HttpProvider(Arc<Inner>);

impl HttpProvider {
    /// 目录里的模型 id（`deepseek/deepseek-flash`）→ 能发请求的 provider。
    /// `region` 只有百炼用（`singapore`；北京为空）。
    pub fn new(
        catalog_id: &str,
        credential: Credential,
        config: &GatewayConfig,
        region: Option<&str>,
    ) -> Result<Self, GatewayError> {
        let model = catalog()
            .model(catalog_id)
            .cloned()
            .ok_or_else(|| GatewayError::new(ErrorKind::BadRequest, format!("模型目录里没有 {catalog_id}。")))?;
        let provider = catalog()
            .provider(&model.provider)
            .cloned()
            .ok_or_else(|| GatewayError::new(ErrorKind::Internal, "模型目录缺了供应商。"))?;
        let vertex = provider.auth == Auth::Adc;
        if vertex && !config.vertex.is_set() {
            return Err(GatewayError::new(
                ErrorKind::Auth,
                "还没填 Vertex 的 Project ID：在设置的「供应商和 Key」里填。",
            ));
        }
        let base = || -> Result<String, GatewayError> {
            region
                .and_then(|r| provider.region_base_urls.get(r))
                .or(provider.base_url.as_ref())
                .map(|b| b.trim_end_matches('/').to_string())
                .ok_or_else(|| GatewayError::new(ErrorKind::Internal, "模型目录缺了地址。"))
        };
        let url = match (provider.wire, vertex) {
            (Wire::Messages, true) => config.vertex.claude_url(&model.api_model),
            (Wire::Gemini, true) => config.vertex.gemini_url(&model.api_model),
            (Wire::Messages, false) => format!("{}/messages", base()?),
            (Wire::Responses, _) => format!("{}/responses", base()?),
            (Wire::Chat, _) => format!("{}/chat/completions", base()?),
            (Wire::Gemini, false) => format!(
                "{}/models/{}:streamGenerateContent?alt=sse",
                base()?,
                model.api_model
            ),
        };
        let origin = Origin {
            provider: provider.id.clone(),
            wire: provider.wire,
            model: model.api_model.clone(),
        };
        Ok(Self(Arc::new(Inner {
            quota_project: vertex.then(|| config.vertex.project_id.clone()),
            provider,
            model,
            origin,
            url,
            credential,
            region: region.map(str::to_string),
            client: client()?,
        })))
    }

    /// 测试用：换成本机的假服务地址。
    #[cfg(test)]
    pub fn at(self, url: &str) -> Self {
        let inner = Arc::try_unwrap(self.0).unwrap_or_else(|_| panic!("还在用"));
        Self(Arc::new(Inner {
            url: url.into(),
            ..inner
        }))
    }

    pub fn model(&self) -> &ModelSpec {
        &self.0.model
    }

    pub fn origin(&self) -> &Origin {
        &self.0.origin
    }

    /// 发出去的 body（不发请求）。
    pub fn body(&self, request: &ModelRequest) -> Value {
        let i = &self.0;
        match i.provider.wire {
            Wire::Messages => anthropic::encode(request, &i.model, &i.origin, i.provider.auth == Auth::Adc),
            Wire::Responses => responses::encode(request, &i.model, &i.origin),
            Wire::Chat => chat::encode(
                request,
                &i.model,
                &i.origin,
                chat::Dialect::from_catalog(i.provider.dialect.as_deref()),
            ),
            Wire::Gemini => gemini::encode(request, &i.model, &i.origin),
        }
    }

    fn stream(&self) -> Box<dyn WireStream> {
        let i = &self.0;
        let id = Some(i.model.id.clone());
        match i.provider.wire {
            Wire::Messages => Box::new(anthropic::AnthropicStream::new(i.origin.clone(), id)),
            Wire::Responses => Box::new(responses::ResponsesStream::new(i.origin.clone(), id)),
            Wire::Chat => Box::new(chat::ChatStream::new(
                i.origin.clone(),
                id,
                i.region.clone(),
                chat::Dialect::from_catalog(i.provider.dialect.as_deref()),
            )),
            Wire::Gemini => Box::new(gemini::GeminiStream::new(i.origin.clone(), id)),
        }
    }

    async fn send(
        self,
        request: ModelRequest,
        events: mpsc::UnboundedSender<ModelEvent>,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<ModelResponse, GatewayError> {
        if *cancel.borrow() {
            return Err(GatewayError::cancelled());
        }
        let body = self.body(&request);
        let recorder = Recorder::start(&self.0.provider.id, &body);
        let i = &self.0;
        let mut req = i
            .client
            .post(&i.url)
            .header("accept", "text/event-stream")
            .json(&body);
        req = match &i.credential {
            Credential::Key(key) => match (i.provider.wire, i.provider.id.as_str()) {
                (Wire::Messages, _) => req
                    .header("x-api-key", key.as_str())
                    .header("anthropic-version", ANTHROPIC_VERSION),
                (Wire::Gemini, _) => req.header("x-goog-api-key", key.as_str()),
                _ => req.bearer_auth(key.as_str()),
            },
            Credential::Adc(token) => {
                let t = token.get().await?;
                let mut r = req.bearer_auth(t.as_str());
                if let Some(p) = &i.quota_project {
                    r = r.header("x-goog-user-project", p);
                }
                r
            }
        };
        let response = tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => return Err(GatewayError::cancelled()),
            r = req.send() => r.map_err(|e| network(&e))?,
        };
        let status = response.status().as_u16();
        if status != 200 {
            let text = tokio::select! {
                biased;
                _ = cancelled(&mut cancel) => return Err(GatewayError::cancelled()),
                t = response.text() => t.unwrap_or_default(),
            };
            if let Some(r) = &recorder {
                r.write_response(text.as_bytes());
            }
            if status == 401 {
                if let Credential::Adc(token) = &i.credential {
                    token.forget().await;
                }
            }
            return Err(http_error(status, &text, &i.provider));
        }
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.starts_with("text/event-stream"));
        if !is_sse {
            return Err(GatewayError::new(ErrorKind::Upstream, "供应商没按流式回答。"));
        }
        let mut response = response;
        let mut framer = SseFramer::default();
        let mut stream = self.stream();
        let mut raw = Vec::new();
        'read: loop {
            let chunk = tokio::select! {
                biased;
                _ = cancelled(&mut cancel) => return Err(GatewayError::cancelled()),
                c = response.chunk() => c.map_err(|e| {
                    if e.is_timeout() {
                        GatewayError::new(ErrorKind::Network, "读回答超时了，没自动重发。")
                    } else {
                        GatewayError::new(ErrorKind::Network, "回答读到一半连接断了，没自动重发。")
                    }
                })?,
            };
            let Some(chunk) = chunk else { break };
            if recorder.is_some() {
                raw.extend_from_slice(&chunk);
            }
            for data in framer.push(&chunk).map_err(|m| GatewayError::new(ErrorKind::Upstream, m))? {
                if data == "[DONE]" {
                    break 'read;
                }
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    return Err(GatewayError::new(ErrorKind::Upstream, "供应商回了读不懂的流。"));
                };
                for e in stream.feed(&value) {
                    let _ = events.send(e);
                }
                if stream.done() {
                    break 'read;
                }
            }
        }
        if let Some(r) = &recorder {
            r.write_response(&raw);
        }
        stream
            .finish()
            .map_err(|m| GatewayError::new(ErrorKind::Upstream, m))
    }
}

impl ModelProvider for HttpProvider {
    fn id(&self) -> &str {
        &self.0.provider.id
    }

    fn wire(&self) -> Wire {
        self.0.provider.wire
    }

    fn complete(
        &self,
        request: ModelRequest,
        events: mpsc::UnboundedSender<ModelEvent>,
        cancel: watch::Receiver<bool>,
    ) -> BoxFuture<Result<ModelResponse, GatewayError>> {
        Box::pin(self.clone().send(request, events, cancel))
    }
}

fn client() -> Result<reqwest::Client, GatewayError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| GatewayError::new(ErrorKind::Internal, "建不了 API 连接。"))
}

/// 等到 `cancel` 变成真。发送端没了就一直等（不当成取消）。
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

fn network(e: &reqwest::Error) -> GatewayError {
    if e.is_timeout() {
        GatewayError::new(ErrorKind::Network, "API 请求超时了，没自动重发。")
    } else {
        GatewayError::new(ErrorKind::Network, "连不上供应商，没自动重发。")
    }
}

/// HTTP 错误 → 给站主看的一句话。供应商自己的说明截一段带上（不含 Key）。
fn http_error(status: u16, body: &str, provider: &ProviderSpec) -> GatewayError {
    let detail = provider_message(body)
        .map(|m| format!("（{}）", super::convert::short(&m, 200)))
        .unwrap_or_default();
    let label = &provider.label;
    let (kind, message) = match status {
        401 | 403 if provider.auth == Auth::Adc => (
            ErrorKind::Auth,
            format!("{label} 拒绝了认证：先在终端跑 gcloud auth application-default login，再看项目有没有开 Vertex AI 和计费{detail}"),
        ),
        401 | 403 => (ErrorKind::Auth, format!("{label} 拒绝了 Key：到设置里看看 Key 和账户权限{detail}")),
        402 => (ErrorKind::RateLimit, format!("{label} 账户余额不足{detail}")),
        429 => (ErrorKind::RateLimit, format!("{label} 限流或额度用完了，没自动重发{detail}")),
        400 | 404 | 422 => (ErrorKind::BadRequest, format!("{label} 拒绝了这次请求{detail}")),
        500..=599 => (ErrorKind::Upstream, format!("{label} 那边出错了（HTTP {status}），没自动重发{detail}")),
        _ => (ErrorKind::Upstream, format!("{label} 返回 HTTP {status}，没自动重发{detail}")),
    };
    let mut e = GatewayError::new(kind, message);
    if status == 402 {
        e.retryable = false;
    }
    e
}

fn provider_message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let v = if v.is_array() { v[0].clone() } else { v };
    v["error"]["message"]
        .as_str()
        .or_else(|| v["message"].as_str())
        .or_else(|| v["error"].as_str())
        .map(str::to_string)
}

/// SSE 按字节切帧：网络包把一个汉字切成两半也不会乱码。一帧的 `data:` 拼起来交出去。
#[derive(Default)]
struct SseFramer {
    buffer: Vec<u8>,
    data: Vec<String>,
}

impl SseFramer {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, String> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > FRAME_LIMIT {
            return Err("流里一帧太大了，停下了。".into());
        }
        let mut out = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| "流的编码不对。".to_string())?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push(self.data.join("\n"));
                    self.data.clear();
                }
            } else if let Some(d) = line.strip_prefix("data:") {
                self.data.push(d.strip_prefix(' ').unwrap_or(d).to_string());
            }
        }
        Ok(out)
    }
}

// ───────────────────────── Vertex 的令牌 ─────────────────────────

/// gcloud ADC 换来的令牌：一小时过期，这里 45 分钟换一次；被拒（401）就扔掉、下次重换。
/// 用 `gcloud auth application-default print-access-token` 换，Peck 不读 ADC 文件的内容。
pub struct AdcToken {
    gcloud: PathBuf,
    cached: tokio::sync::Mutex<Option<(Zeroizing<String>, Instant)>>,
}

impl AdcToken {
    pub fn new(gcloud: PathBuf) -> Self {
        Self {
            gcloud,
            cached: tokio::sync::Mutex::new(None),
        }
    }

    /// 在设置的「CLI 目录」里找 gcloud，没有就照 PATH 找。
    pub async fn locate(config: &GatewayConfig) -> Self {
        let extra: Vec<String> = config
            .cli_dir_path()
            .map(|p| vec![p.to_string_lossy().into_owned()])
            .unwrap_or_default();
        let path = crate::agent::build_augmented_path(&extra);
        let gcloud = crate::agent::resolve_command_path("gcloud", &path)
            .await
            .unwrap_or_else(|| PathBuf::from("gcloud"));
        Self::new(gcloud)
    }

    pub async fn get(&self) -> Result<Zeroizing<String>, GatewayError> {
        let mut cached = self.cached.lock().await;
        if let Some((token, at)) = cached.as_ref() {
            if at.elapsed() < Duration::from_secs(45 * 60) {
                return Ok(token.clone());
            }
        }
        let run = tokio::process::Command::new(&self.gcloud)
            .args(["auth", "application-default", "print-access-token"])
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let output = tokio::time::timeout(Duration::from_secs(30), run)
            .await
            .map_err(|_| GatewayError::new(ErrorKind::Auth, "gcloud 换令牌超时了。"))?
            .map_err(|_| GatewayError::new(ErrorKind::Auth, "找不到 gcloud：装好以后在设置里填「CLI 目录」。"))?;
        let token = Zeroizing::new(String::from_utf8_lossy(&output.stdout).trim().to_string());
        if !output.status.success() || token.is_empty() {
            return Err(GatewayError::new(
                ErrorKind::Auth,
                "gcloud 换不到令牌：先在终端跑 gcloud auth application-default login。",
            ));
        }
        *cached = Some((token.clone(), Instant::now()));
        Ok(token)
    }

    pub async fn forget(&self) {
        *self.cached.lock().await = None;
    }
}

// ───────────────────────── 录夹具 ─────────────────────────

struct Recorder {
    stem: PathBuf,
}

impl Recorder {
    fn start(provider: &str, body: &Value) -> Option<Self> {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = PathBuf::from(std::env::var_os("PECK_RECORD_DIR")?);
        std::fs::create_dir_all(&dir).ok()?;
        let n = N.fetch_add(1, Ordering::SeqCst) + 1;
        let stem = dir.join(format!("{provider}-{n}"));
        let text = serde_json::to_string_pretty(body).ok()?;
        std::fs::write(stem.with_extension("request.json"), text + "\n").ok()?;
        Some(Self { stem })
    }

    fn write_response(&self, bytes: &[u8]) {
        let _ = std::fs::write(self.stem.with_extension("sse"), bytes);
    }
}

// ───────────────────────── 从 App 的状态拿 ─────────────────────────

/// 发任务时用：目录里的模型 id → provider。Key 从钥匙串拿；Vertex 用 ADC；
/// 百炼的地域照「供应商和 Key」里的设置。模拟模式给假模型（照剧本出工具调用）。
pub async fn for_model(
    state: &crate::state::AppState,
    catalog_id: &str,
) -> Result<Arc<dyn ModelProvider>, GatewayError> {
    fn internal<E>(_: E) -> GatewayError {
        GatewayError::new(ErrorKind::Internal, "读不到设置。")
    }
    if *state.api.mode.lock().map_err(internal)? == crate::mode::ApiMode::Mock {
        return Ok(Arc::new(super::mock::ScriptedModel::peck_demo(Duration::from_millis(400))));
    }
    let spec = catalog()
        .model(catalog_id)
        .ok_or_else(|| GatewayError::new(ErrorKind::BadRequest, format!("模型目录里没有 {catalog_id}。")))?;
    let provider = catalog()
        .provider(&spec.provider)
        .ok_or_else(|| GatewayError::new(ErrorKind::Internal, "模型目录缺了供应商。"))?;
    let (config, region) = {
        let store = state.store.lock().map_err(internal)?;
        let config = GatewayConfig::load(&store).map_err(internal)?;
        let region = store
            .api_config()
            .ok()
            .and_then(|c| match c.aliyun_region {
                crate::api::profiles::AliyunRegion::Singapore => Some("singapore".to_string()),
                crate::api::profiles::AliyunRegion::Beijing => None,
            })
            .filter(|_| provider.id == "aliyun");
        (config, region)
    };
    let credential = match provider.auth {
        Auth::Key => {
            let key = state
                .api
                .keys
                .lock()
                .map_err(internal)?
                .get(&provider.id)
                .map_err(|_| {
                    GatewayError::new(
                        ErrorKind::Auth,
                        format!("还没填 {} 的 Key：在设置的「供应商和 Key」里填。", provider.label),
                    )
                })?;
            Credential::Key(key)
        }
        Auth::Adc => Credential::Adc(adc_token(&config).await),
    };
    Ok(Arc::new(HttpProvider::new(catalog_id, credential, &config, region.as_deref())?))
}

/// 整个 App 共用一个 ADC 令牌（换了 CLI 目录就重找 gcloud）。
async fn adc_token(config: &GatewayConfig) -> Arc<AdcToken> {
    static TOKEN: std::sync::Mutex<Option<(String, Arc<AdcToken>)>> = std::sync::Mutex::new(None);
    if let Ok(guard) = TOKEN.lock() {
        if let Some((dir, token)) = guard.as_ref() {
            if *dir == config.cli_dir {
                return token.clone();
            }
        }
    }
    let token = Arc::new(AdcToken::locate(config).await);
    if let Ok(mut guard) = TOKEN.lock() {
        *guard = Some((config.cli_dir.clone(), token.clone()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::{Message, Part, Role};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn ask(model: &str) -> ModelRequest {
        ModelRequest {
            model: model.into(),
            system: vec![],
            messages: vec![Message {
                role: Role::User,
                parts: vec![Part::Text { text: "你好".into() }],
            }],
            tools: vec![],
            thinking: false,
            effort: None,
            max_output_tokens: 100,
        }
    }

    /// 起一个只回一次的假服务：收下请求（含头），回 `status` 和 `body`，切成小段发。
    async fn serve_once(status: &str, content_type: &str, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_string();
        let content_type = content_type.to_string();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            loop {
                let mut b = [0; 4096];
                let n = socket.read(&mut b).await.unwrap();
                seen.extend_from_slice(&b[..n]);
                if let Some(head_end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&seen[..head_end]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if seen.len() >= head_end + 4 + len {
                        break;
                    }
                }
            }
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            socket.write_all(head.as_bytes()).await.unwrap();
            for c in body.as_bytes().chunks(5) {
                socket.write_all(c).await.unwrap();
            }
            String::from_utf8_lossy(&seen).into_owned()
        });
        (format!("http://{addr}/x"), handle)
    }

    fn key(k: &str) -> Credential {
        Credential::Key(Zeroizing::new(k.into()))
    }

    #[tokio::test]
    async fn chat_stream_over_http_with_bearer_and_done_marker() {
        let body = "data: {\"model\":\"deepseek-flash\",\"choices\":[{\"delta\":{\"content\":\"中文回答\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
        let (url, server) = serve_once("200 OK", "text/event-stream", body).await;
        let p = HttpProvider::new("deepseek/deepseek-flash", key("sk-test"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&url);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        let r = p.complete(ask("deepseek/deepseek-flash"), tx, cancel).await.unwrap();
        assert_eq!(r.message.parts, vec![Part::Text { text: "中文回答".into() }]);
        assert_eq!(r.usage.input, Some(5));
        assert_eq!(rx.recv().await, Some(ModelEvent::TextDelta { text: "中文回答".into() }));
        let seen = server.await.unwrap();
        assert!(seen.to_lowercase().contains("authorization: bearer sk-test"));
        assert!(seen.contains("\"thinking\":{\"type\":\"disabled\"}"));
    }

    #[tokio::test]
    async fn anthropic_and_gemini_use_their_own_key_headers() {
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"model\":\"m\",\"usage\":{\"input_tokens\":3}}}\n\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"好\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        let (url, server) = serve_once("200 OK", "text/event-stream", sse).await;
        let p = HttpProvider::new("anthropic/claude-haiku-5-5", key("ak"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&url);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        let r = p.complete(ask("anthropic/claude-haiku-5-5"), tx, cancel).await.unwrap();
        assert_eq!(r.stop, crate::gateway::StopReason::EndTurn);
        let seen = server.await.unwrap().to_lowercase();
        assert!(seen.contains("x-api-key: ak") && seen.contains(&format!("anthropic-version: {ANTHROPIC_VERSION}")));

        let sse = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"好\"}]},\"finishReason\":\"STOP\"}]}\r\n\r\n";
        let (url, server) = serve_once("200 OK", "text/event-stream", sse).await;
        let p = HttpProvider::new("google/gemini-3.5-flash-lite", key("gk"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&url);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        p.complete(ask("google/gemini-3.5-flash-lite"), tx, cancel).await.unwrap();
        let seen = server.await.unwrap().to_lowercase();
        assert!(seen.contains("x-goog-api-key: gk") && !seen.contains("authorization"));
    }

    #[tokio::test]
    async fn errors_become_plain_chinese_without_the_key() {
        let (url, _server) = serve_once("401 Unauthorized", "application/json", "{\"error\":{\"message\":\"invalid x-api-key\"}}").await;
        let p = HttpProvider::new("openai/gpt-6-luna", key("sk-secret-value"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&url);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        let e = p.complete(ask("openai/gpt-6-luna"), tx, cancel).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Auth);
        assert!(e.message.contains("OpenAI") && e.message.contains("invalid x-api-key"));
        assert!(!e.message.contains("sk-secret-value"));

        let (url, _server) = serve_once("429 Too Many Requests", "application/json", "[{\"error\":{\"message\":\"quota\"}}]").await;
        let p = HttpProvider::new("google/gemini-3.5-flash-lite", key("k"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&url);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        let e = p.complete(ask("google/gemini-3.5-flash-lite"), tx, cancel).await.unwrap_err();
        assert!(e.retryable && e.message.contains("quota"));
    }

    #[tokio::test]
    async fn stopping_returns_promptly_and_dropping_the_sender_does_not_cancel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut b = [0; 4096];
            let _ = socket.read(&mut b).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let p = HttpProvider::new("openai/gpt-6-luna", key("k"), &GatewayConfig::default(), None)
            .unwrap()
            .at(&format!("http://{addr}/x"));
        let (tx, _rx) = mpsc::unbounded_channel();
        let (stop, cancel) = watch::channel(false);
        let task = tokio::spawn(p.complete(ask("openai/gpt-6-luna"), tx, cancel));
        tokio::time::sleep(Duration::from_millis(200)).await;
        stop.send(true).unwrap();
        let r = tokio::time::timeout(Duration::from_millis(500), task).await.unwrap().unwrap();
        assert_eq!(r.unwrap_err().kind, ErrorKind::Cancelled);
        server.abort();

        // 发送端没了：照常等，不当成取消。
        let (stop, mut cancel) = watch::channel(false);
        drop(stop);
        let waited = tokio::time::timeout(Duration::from_millis(100), cancelled(&mut cancel)).await;
        assert!(waited.is_err());
    }

    #[test]
    fn urls_follow_wire_auth_and_region() {
        let cfg = GatewayConfig::default();
        let url = |id: &str, region: Option<&str>| HttpProvider::new(id, key("k"), &cfg, region).unwrap().0.url.clone();
        assert_eq!(url("anthropic/claude-haiku-5-5", None), "https://api.anthropic.com/v1/messages");
        assert_eq!(url("openai/gpt-6-luna", None), "https://api.openai.com/v1/responses");
        assert_eq!(url("deepseek/deepseek-flash", None), "https://api.deepseek.com/chat/completions");
        assert_eq!(url("aliyun/qwen3.8-flash", None), "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions");
        assert_eq!(url("aliyun/qwen3.8-flash", Some("singapore")), "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions");
        assert_eq!(
            url("google/gemini-3.5-flash-lite", None),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash-lite:streamGenerateContent?alt=sse"
        );
        // Vertex：没填项目先拦下；填了按项目拼，body 不放 model、带 anthropic_version。
        let adc = || Credential::Adc(Arc::new(AdcToken::new("gcloud".into())));
        let e = HttpProvider::new("vertex-anthropic/claude-haiku-5-5@default", adc(), &cfg, None).err().unwrap();
        assert_eq!(e.kind, ErrorKind::Auth);
        let mut cfg = GatewayConfig::default();
        cfg.vertex.project_id = "my-project-1".into();
        let p = HttpProvider::new("vertex-anthropic/claude-haiku-5-5@default", adc(), &cfg, None).unwrap();
        assert!(p.0.url.ends_with("/publishers/anthropic/models/claude-haiku-5-5@default:streamRawPredict"));
        let body = p.body(&ask("vertex-anthropic/claude-haiku-5-5@default"));
        assert!(body.get("model").is_none());
        assert_eq!(body["anthropic_version"], anthropic::VERTEX_VERSION);
        let p = HttpProvider::new("vertex-google/gemini-3.8-flash", adc(), &cfg, None).unwrap();
        assert!(p.0.url.ends_with("/publishers/google/models/gemini-3.8-flash:streamGenerateContent?alt=sse"));
        assert_eq!(p.0.quota_project.as_deref(), Some("my-project-1"));
        let _ = json!(null);
    }

    /// 真请求（本地录夹具用，平时不跑）：一次让模型调工具，一次带工具结果回放。花两次授权。
    /// `PECK_LIVE_KEYS=<Key 目录> PECK_LIVE_MODEL=openai/gpt-6-luna PECK_RECORD_DIR=<输出>
    ///  cargo test --lib live_two_requests -- --ignored --nocapture`
    /// Key 文件名照供应商（Gemini 是 `gemini`）；只打印停止原因、块的种类和用量，不打印 Key。
    #[tokio::test]
    #[ignore]
    async fn live_two_requests() {
        let model_id = std::env::var("PECK_LIVE_MODEL").expect("PECK_LIVE_MODEL");
        let keys = PathBuf::from(std::env::var("PECK_LIVE_KEYS").expect("PECK_LIVE_KEYS"));
        let region = std::env::var("PECK_LIVE_REGION").ok();
        let effort = std::env::var("PECK_LIVE_EFFORT").ok().filter(|e| !e.is_empty());
        let spec = catalog().model(&model_id).unwrap();
        let file = if spec.provider == "google" { "gemini" } else { spec.provider.as_str() };
        let key = Zeroizing::new(std::fs::read_to_string(keys.join(file)).unwrap().trim().to_string());
        let p = HttpProvider::new(&model_id, Credential::Key(key), &GatewayConfig::default(), region.as_deref()).unwrap();
        let mut request = live_request(&model_id, effort);
        let kinds = |m: &Message| {
            m.parts
                .iter()
                .map(|p| match p {
                    Part::Text { .. } => "text",
                    Part::Reasoning { .. } => "reasoning",
                    Part::ToolCall { .. } => "tool_call",
                    Part::ToolResult { .. } => "tool_result",
                })
                .collect::<Vec<_>>()
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        let first = p.complete(request.clone(), tx, cancel).await;
        let first = match first {
            Ok(r) => r,
            Err(e) => panic!("第一次失败：{e}"),
        };
        println!("req1: {:?} {:?} usage={:?} cost={:?}", first.stop, kinds(&first.message), first.usage, first.cost);
        let Some(call_id) = first.message.parts.iter().find_map(|p| match p {
            Part::ToolCall { call_id, .. } => Some(call_id.clone()),
            _ => None,
        }) else {
            println!("没调工具，只发了一次");
            return;
        };
        request.messages.push(first.message.clone());
        request.messages.push(Message {
            role: Role::User,
            parts: vec![Part::ToolResult {
                call_id,
                output: LIVE_NOTES
                    .lines()
                    .enumerate()
                    .map(|(i, l)| format!("{:6}\t{l}\n", i + 1))
                    .collect(),
                is_error: false,
            }],
        });
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_c, cancel) = watch::channel(false);
        match p.complete(request, tx, cancel).await {
            Ok(r) => println!("req2: {:?} {:?} usage={:?} cost={:?}", r.stop, kinds(&r.message), r.usage, r.cost),
            Err(e) => panic!("第二次失败：{e}"),
        }
    }

}

/// 录 API 夹具用的小任务（和 Anthropic 那份一样）：notes.txt 里有几条。
#[cfg(test)]
pub(crate) const LIVE_NOTES: &str = "# 笔记\n- 第一行\n- 第二行\n- 测试通过\n";

#[cfg(test)]
pub(crate) fn live_request(model_id: &str, effort: Option<String>) -> ModelRequest {
    use crate::gateway::{Message, Part, Role, SystemBlock, ToolSpec};
    ModelRequest {
        model: model_id.into(),
        system: vec![SystemBlock {
            text: "You are a coding agent in a small project. Read files with the tool before answering. Answer briefly in Chinese.".into(),
            cache: false,
        }],
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Text {
                text: "notes.txt 里有几条以「- 」开头的条目？".into(),
            }],
        }],
        tools: vec![ToolSpec {
            name: "read_file".into(),
            description: "Read a text file in the project. Returns its lines with line numbers.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "Path relative to the project root"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        }],
        thinking: true,
        effort,
        max_output_tokens: 2000,
    }
}

#[cfg(test)]
mod fixture_tests {
    //! 四家 API 的真实录音（`tests/fixtures/gateway/api-*-tools/`）：第一次让模型调工具，第二次带结果回放。
    //! 验三件事：Peck 拼的第一次请求和真发出去的一样；第一次的流解析成什么（`req1.expected.json`）；
    //! 把解析出的回答放回去，拼出的第二次请求和真发出去、被供应商接受的那份一字不差。
    use super::*;
    use crate::gateway::convert::fixtures;
    use crate::gateway::{Message, Part, Role};
    use serde_json::json;

    fn provider(model: &str, region: Option<&str>) -> HttpProvider {
        HttpProvider::new(model, Credential::Key(Zeroizing::new("k".into())), &GatewayConfig::default(), region).unwrap()
    }

    fn parse(p: &HttpProvider, sse: &str) -> (Vec<ModelEvent>, ModelResponse) {
        let mut framer = SseFramer::default();
        let mut stream = p.stream();
        let mut events = Vec::new();
        for data in framer.push(sse.as_bytes()).unwrap() {
            if data == "[DONE]" {
                break;
            }
            events.extend(stream.feed(&serde_json::from_str(&data).unwrap()));
        }
        (events, stream.finish().unwrap())
    }

    fn check_json(name: &str, file: &str, actual: &Value) {
        let path = fixtures::dir(name).join(file);
        if std::env::var_os("PECK_REGEN_FIXTURES").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(actual).unwrap() + "\n").unwrap();
            return;
        }
        let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // 小数读回来可能差最后一位：实际结果也写成文本再读回来，两边走同一条路。
        let actual: Value = serde_json::from_str(&serde_json::to_string(actual).unwrap()).unwrap();
        assert_eq!(expected, actual, "{name}/{file} 不一样");
    }

    fn recorded(name: &str, file: &str) -> Value {
        serde_json::from_str(&fixtures::read(name, file)).unwrap()
    }

    fn round_trip(name: &str, model: &str, region: Option<&str>, effort: Option<&str>) -> (ModelResponse, ModelResponse) {
        let p = provider(model, region);
        let mut request = live_request(model, effort.map(str::to_string));
        assert_eq!(p.body(&request), recorded(name, "req1.request.json"), "{name}：第一次请求变了");

        let (events, first) = parse(&p, &fixtures::read(name, "req1.sse"));
        check_json(name, "req1.expected.json", &json!({"events": events, "response": first}));
        let call_id = first
            .message
            .parts
            .iter()
            .find_map(|p| match p {
                Part::ToolCall { call_id, .. } => Some(call_id.clone()),
                _ => None,
            })
            .expect("第一次应该调工具");
        request.messages.push(first.message.clone());
        request.messages.push(Message {
            role: Role::User,
            parts: vec![Part::ToolResult {
                call_id,
                output: LIVE_NOTES
                    .lines()
                    .enumerate()
                    .map(|(i, l)| format!("{:6}\t{l}\n", i + 1))
                    .collect(),
                is_error: false,
            }],
        });
        assert_eq!(p.body(&request), recorded(name, "req2.request.json"), "{name}：回放拼出的第二次请求和真发的不一样");

        let (events, second) = parse(&p, &fixtures::read(name, "req2.sse"));
        check_json(name, "req2.expected.json", &json!({"events": events, "response": second}));
        assert_eq!(first.stop, crate::gateway::StopReason::ToolUse);
        assert_eq!(second.stop, crate::gateway::StopReason::EndTurn);
        assert!(second.message.parts.iter().any(|p| matches!(p, Part::Text { text } if text.contains('3'))));
        assert_eq!(first.cost.source, crate::gateway::event::CostSource::Priced);
        (first, second)
    }

    #[test]
    fn openai_responses_fixture() {
        let (first, _) = round_trip("api-openai-tools", "openai/gpt-6-luna", None, Some("high"));
        assert_eq!(first.usage.cache_write, Some(0));
    }

    #[test]
    fn deepseek_chat_fixture() {
        let (_, second) = round_trip("api-deepseek-tools", "deepseek/deepseek-flash", None, Some("high"));
        assert!(matches!(&second.message.parts[0], Part::Reasoning { .. }));
        assert!(second.usage.cache_read.unwrap() > 0, "第二次应该命中缓存");
    }

    #[test]
    fn aliyun_chat_fixture() {
        let (first, _) = round_trip("api-aliyun-tools", "aliyun/qwen3.8-flash", None, None);
        assert!(matches!(&first.message.parts[0], Part::Reasoning { .. }));
        // 百炼不放回思考：第二次请求里没有 reasoning_content。
        let sent = recorded("api-aliyun-tools", "req2.request.json");
        assert!(sent["messages"][2].get("reasoning_content").is_none());
    }

    #[test]
    fn gemini_native_fixture() {
        let (first, second) = round_trip("api-gemini-tools", "google/gemini-3.5-flash-lite", None, Some("low"));
        let Part::ToolCall { call_id, signature, .. } = &first.message.parts[0] else {
            panic!("第一块是工具调用");
        };
        assert!(!call_id.starts_with(MADE_UP));
        assert!(signature.is_some(), "工具调用要带签名");
        assert!(matches!(&second.message.parts[1], Part::Reasoning { text, .. } if text.is_empty()));
    }

    const MADE_UP: &str = "peck-call-";
}

#[cfg(test)]
mod framer_tests {
    use super::*;

    #[test]
    fn sse_frames_survive_split_bytes_and_crlf() {
        let mut f = SseFramer::default();
        let text = "data: {\"a\":\"中文\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
        let mut got = Vec::new();
        for c in text.chunks(3) {
            got.extend(f.push(c).unwrap());
        }
        assert_eq!(got, vec!["{\"a\":\"中文\"}".to_string(), "[DONE]".to_string()]);
    }
}
