//! 内置预览面板（v0.8 Phase 12）的本机代理：右栏 PREVIEW 不直接开 dev server 的地址，而是开 Peck 在
//! `127.0.0.1` 上起的一个小代理。代理把请求原样转给 dev server（只认本机地址），回来的 HTML 里插一段小脚本
//! （`preview.js`：点元素写批注），别的原样回去；WebSocket（Vite 的热更新这类）升级以后两头直接对接。
//!
//! 每条连接一个任务、只办一个请求（两头都是 `Connection: close`，浏览器要并发就多开几条连接）。回应边读边转、
//! 不攒整份：SSE、分块的流式输出、长轮询来一段到一段，到声明的长度、分块的结尾或者上游断开为止；请求的正文也是
//! 边读边转。浏览器那头一断开，上游的连接跟着关。
//!
//! 为了能插脚本：请求去掉 `Accept-Encoding`（回来的不压缩）；HTML 去掉 `Content-Security-Policy`、
//! `X-Frame-Options`（不然放不进面板、脚本跑不了），分块的解开，攒到看见 `</head>`（或 `<body`）插好脚本就放行，
//! 后面的照样边读边转——上游给了长度就加上脚本的长度，没给就以断开连接作结束。只为开发时自己看，不做缓存、不做 HTTPS。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const SCRIPT: &str = include_str!("preview.js");
const MAX_HEAD: usize = 64 * 1024;
/// 一次读写多大的一段。
const PIECE: usize = 16 * 1024;
/// HTML 攒着等 `</head>`，最多攒这么多，再多就不等了。
const MAX_HOLD: usize = 1024 * 1024;
/// HTML 攒着等 `</head>` 的时候，上游停这么久没动静就先放行（没有 `<head>`、一点点吐的流式页面）。
const HOLD_WAIT: Duration = Duration::from_millis(300);

/// 要预览的地址：只认本机的 http。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `localhost`、`127.0.0.1`、`[::1]`。
    pub host: String,
    pub port: u16,
    /// 路径和查询（`/`、`/docs?x=1`）。
    pub path: String,
}

impl Target {
    pub fn parse(url: &str) -> Result<Self, String> {
        let rest = url.trim().strip_prefix("http://").ok_or("预览只认本机的 http 地址（比如 http://localhost:5173）。")?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = if let Some(h) = authority.strip_prefix("[::1]") {
            ("[::1]".to_string(), h.strip_prefix(':').unwrap_or("80"))
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_lowercase(), p),
                None => (authority.to_lowercase(), "80"),
            }
        };
        if !matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") {
            return Err("预览只开本机的地址（localhost、127.0.0.1），不代理外面的网站。".into());
        }
        let port: u16 = port.parse().map_err(|_| "地址里的端口不对。".to_string())?;
        Ok(Self { host: if host == "0.0.0.0" { "127.0.0.1".into() } else { host }, port, path: path.to_string() })
    }

    fn origin(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn connect_host(&self) -> &str {
        match self.host.as_str() {
            "localhost" => "127.0.0.1",
            "[::1]" => "::1",
            h => h,
        }
    }
}

fn proxies() -> &'static Mutex<HashMap<String, u16>> {
    static P: OnceLock<Mutex<HashMap<String, u16>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 给这个 dev server 起（或者拿已有的）代理，返回面板要开的地址（路径照原来的）。
pub async fn start(url: &str) -> Result<String, String> {
    let target = Target::parse(url)?;
    if let Some(port) = proxies().lock().ok().and_then(|p| p.get(&target.origin()).copied()) {
        return Ok(format!("http://127.0.0.1:{port}{}", target.path));
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("起不了预览代理：{e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    if let Ok(mut p) = proxies().lock() {
        p.insert(target.origin(), port);
    }
    let t = target.clone();
    tokio::spawn(async move {
        loop {
            // 一时接不了（比如句柄用完了）：歇一下再接，代理不就此停掉。
            let Ok((client, _)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            let t = t.clone();
            tokio::spawn(async move {
                let _ = handle(client, &t, port).await;
            });
        }
    });
    Ok(format!("http://127.0.0.1:{port}{}", target.path))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn find_ignore_case(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

/// 读到请求头（或回应头）结束；返回（头，头后面已经读到的字节）。
async fn read_head<R: AsyncRead + Unpin>(stream: &mut R) -> Result<(Vec<u8>, Vec<u8>), String> {
    read_head_from(stream, Vec::with_capacity(4096)).await
}

/// 同上，`buf` 是之前已经读到的。
async fn read_head_from<R: AsyncRead + Unpin>(stream: &mut R, mut buf: Vec<u8>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            let rest = buf.split_off(i + 4);
            return Ok((buf, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err("头太长。".into());
        }
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("连接断了。".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn split_head(head: &[u8]) -> (String, Vec<(String, String)>) {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n").filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or("").to_string();
    let headers = lines
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect();
    (first, headers)
}

fn join_head(first: &str, headers: &[(String, String)]) -> Vec<u8> {
    let mut out = format!("{first}\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// 分块传输的增量解析：喂进来的字节里哪些是正文、到哪结束。
#[derive(Default)]
struct Chunked {
    state: ChunkState,
    /// 正在读的那一行（块的长度行、拖尾的头）。
    line: Vec<u8>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ChunkState {
    /// 等一行块长度。
    #[default]
    Size,
    /// 块里还剩这么多正文。
    Data(u64),
    /// 块正文后面的换行。
    DataEnd,
    /// 最后一块（长度 0）之后的拖尾头，到空行为止。
    Trailer,
    Done,
}

impl Chunked {
    /// 喂一段，返回用掉了多少字节（比 `input` 短就是正文到头了）；给了 `out` 就把解开的正文接在后面。
    fn feed(&mut self, input: &[u8], mut out: Option<&mut Vec<u8>>) -> usize {
        let mut at = 0;
        while at < input.len() {
            match self.state {
                ChunkState::Done => break,
                ChunkState::Data(left) => {
                    let n = (input.len() - at).min(usize::try_from(left).unwrap_or(usize::MAX));
                    if let Some(out) = out.as_deref_mut() {
                        out.extend_from_slice(&input[at..at + n]);
                    }
                    at += n;
                    self.state = if left == n as u64 { ChunkState::DataEnd } else { ChunkState::Data(left - n as u64) };
                }
                state => {
                    let byte = input[at];
                    at += 1;
                    if byte != b'\n' {
                        if byte != b'\r' && self.line.len() < 64 {
                            self.line.push(byte);
                        }
                        continue;
                    }
                    self.state = match state {
                        ChunkState::DataEnd => ChunkState::Size,
                        ChunkState::Size => {
                            let text = String::from_utf8_lossy(&self.line);
                            // 长度读不懂就当到头了。
                            match u64::from_str_radix(text.split(';').next().unwrap_or("").trim(), 16) {
                                Ok(0) => ChunkState::Trailer,
                                Ok(size) => ChunkState::Data(size),
                                Err(_) => ChunkState::Done,
                            }
                        }
                        _ if self.line.is_empty() => ChunkState::Done,
                        _ => ChunkState::Trailer,
                    };
                    self.line.clear();
                }
            }
        }
        at
    }

    fn done(&self) -> bool {
        self.state == ChunkState::Done
    }
}

fn script_tag() -> String {
    format!("<script data-peck=\"preview\">{SCRIPT}</script>")
}

/// 脚本该插的地方：`</head>` 前面，没有就 `<body` 前面。
fn marker(html: &[u8]) -> Option<usize> {
    find_ignore_case(html, b"</head>").or_else(|| find_ignore_case(html, b"<body"))
}

/// 两个都没有时插在哪：最前，只让过开头的空白、注释和 `<!doctype>`（脚本排到 doctype 前面，页面会掉进怪异模式）。
fn prolog_end(html: &[u8]) -> usize {
    let mut at = if html.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    loop {
        while html.get(at).is_some_and(|b| b.is_ascii_whitespace()) {
            at += 1;
        }
        let rest = &html[at..];
        if rest.starts_with(b"<!--") {
            match find(rest, b"-->") {
                Some(i) => at += i + 3,
                None => return at,
            }
        } else if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case(b"<!doctype") {
            return find(rest, b">").map_or(at, |i| at + i + 1);
        } else {
            return at;
        }
    }
}

/// 把脚本插进 HTML：`</head>` 前面，没有就 `<body` 前面，再没有就放最前（`<!doctype>` 后面）。
pub fn inject(html: &[u8]) -> Vec<u8> {
    let tag = script_tag();
    let at = marker(html).unwrap_or_else(|| prolog_end(html));
    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

/// 边转边插脚本：HTML 先攒着，看见 `</head>` 或 `<body` 就插好放行，之后的原样过。脚本只插一次。
#[derive(Default)]
struct Injector {
    held: Vec<u8>,
    done: bool,
}

impl Injector {
    /// 进来一段，返回现在可以发出去的。
    fn push(&mut self, data: &[u8]) -> Vec<u8> {
        if self.done {
            return data.to_vec();
        }
        self.held.extend_from_slice(data);
        if marker(&self.held).is_some() || self.held.len() > MAX_HOLD {
            self.flush()
        } else {
            Vec::new()
        }
    }

    /// 不再等了：手里的插好脚本全放出去（已经插过就什么都没有）。
    fn flush(&mut self) -> Vec<u8> {
        if std::mem::replace(&mut self.done, true) {
            return Vec::new();
        }
        inject(&std::mem::take(&mut self.held))
    }

    /// 手里攒着东西、还在等插的地方。
    fn waiting(&self) -> bool {
        !self.done && !self.held.is_empty()
    }
}

/// 回应的正文到哪结束。
enum Body {
    /// 没有正文（HEAD、204、304）。
    None,
    /// `Content-Length`：还剩这么多。
    Length(u64),
    Chunked(Chunked),
    /// 都没写：到上游断开为止。
    Close,
}

/// 把回应的正文边读边转给浏览器：读一段写一段，到 `body` 说的结尾或者上游断开为止。`early` 是跟着回应头一起读到的那段。
/// 给了 `injector` 的是 HTML：分块的解开，插好脚本再发；别的字节原样过（分块的连同分块格式一起）。
async fn relay<R, W>(from: &mut R, to: &mut W, early: Vec<u8>, mut body: Body, mut injector: Option<Injector>) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut n = early.len();
    let mut buf = early;
    buf.resize(n.max(PIECE), 0);
    let mut piece = Vec::new();
    loop {
        // 这一段里要发出去的。
        let data = &buf[..n];
        piece.clear();
        let ended = match &mut body {
            Body::None => true,
            Body::Close => {
                piece.extend_from_slice(data);
                false
            }
            Body::Length(left) => {
                let take = data.len().min(usize::try_from(*left).unwrap_or(usize::MAX));
                piece.extend_from_slice(&data[..take]);
                *left -= take as u64;
                *left == 0
            }
            Body::Chunked(chunks) => {
                if injector.is_some() {
                    chunks.feed(data, Some(&mut piece));
                } else {
                    let used = chunks.feed(data, None);
                    piece.extend_from_slice(&data[..used]);
                }
                chunks.done()
            }
        };
        if let Some(injector) = injector.as_mut() {
            piece = injector.push(&piece);
        }
        to.write_all(&piece).await?;
        if ended {
            break;
        }
        n = match injector.as_mut().filter(|i| i.waiting()) {
            // 攒着 HTML 等 `</head>`：上游停了一会儿就不等了，先把手里的放行。
            Some(injector) => match tokio::time::timeout(HOLD_WAIT, from.read(&mut buf)).await {
                Ok(read) => read?,
                Err(_) => {
                    to.write_all(&injector.flush()).await?;
                    from.read(&mut buf).await?
                }
            },
            None => from.read(&mut buf).await?,
        };
        if n == 0 {
            break;
        }
    }
    if let Some(injector) = injector.as_mut() {
        to.write_all(&injector.flush()).await?;
    }
    to.flush().await
}

/// 读上游的回应头，改好发给浏览器，再把正文转过去。
async fn respond<R, W>(from: &mut R, to: &mut W, is_head: bool) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut early = Vec::new();
    let (status, code, mut headers, early) = loop {
        let (head, rest) = read_head_from(from, early).await?;
        let (status, headers) = split_head(&head);
        let code: u16 = status.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        // 1xx（100 Continue、103 Early Hints）只是先打个招呼：原样转过去，正式的回应在后面。
        if (100..200).contains(&code) {
            to.write_all(&head).await.map_err(|e| e.to_string())?;
            early = rest;
            continue;
        }
        break (status, code, headers, rest);
    };
    let chunked = header(&headers, "transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    let length = header(&headers, "content-length").and_then(|v| v.parse::<u64>().ok());
    let body = if is_head || matches!(code, 204 | 304) {
        Body::None
    } else if chunked {
        Body::Chunked(Chunked::default())
    } else if let Some(length) = length {
        Body::Length(length)
    } else {
        Body::Close
    };
    let html = header(&headers, "content-type").is_some_and(|t| t.to_ascii_lowercase().contains("text/html"));
    let injector = (html && (200..300).contains(&code) && !matches!(body, Body::None)).then(Injector::default);
    // 这条连接只办这一个回应。
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("connection") && !k.eq_ignore_ascii_case("keep-alive"));
    if injector.is_some() {
        headers.retain(|(k, _)| {
            let k = k.to_ascii_lowercase();
            !matches!(k.as_str(), "content-length" | "transfer-encoding" | "content-security-policy" | "content-security-policy-report-only" | "x-frame-options")
        });
        // 上游给了长度：加上脚本的。没给的（分块的、靠断开结束的）一律以断开连接作结束。
        if let Body::Length(length) = body {
            headers.push(("Content-Length".into(), (length + script_tag().len() as u64).to_string()));
        }
    }
    headers.push(("Connection".into(), "close".into()));
    to.write_all(&join_head(&status, &headers)).await.map_err(|e| e.to_string())?;
    relay(from, to, early, body, injector).await.map_err(|e| e.to_string())
}

async fn handle(mut client: TcpStream, target: &Target, port: u16) -> Result<(), String> {
    let (head, early) = read_head(&mut client).await?;
    let (first, mut headers) = split_head(&head);
    // 只认发给这个代理自己的请求：别的 Host（DNS rebinding 的网页）一律 403，不替它们绕过 dev server 的检查。
    let own = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !header(&headers, "host").is_some_and(|h| own.iter().any(|o| h.eq_ignore_ascii_case(o))) {
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return Ok(());
    }
    let upgrade = header(&headers, "upgrade").is_some();
    let is_head = first.starts_with("HEAD ");
    // Host 换成 dev server 自己的（Vite 这类会查）；Origin 只有是预览面板自己的才换，别的原样交给 dev server 判断。不压缩、一条连接一个请求。
    for (k, v) in headers.iter_mut() {
        if k.eq_ignore_ascii_case("host") {
            *v = target.origin();
        } else if k.eq_ignore_ascii_case("origin") && own.iter().any(|o| v.eq_ignore_ascii_case(&format!("http://{o}"))) {
            *v = format!("http://{}", target.origin());
        }
    }
    if !upgrade {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("accept-encoding") && !k.eq_ignore_ascii_case("connection") && !k.eq_ignore_ascii_case("keep-alive"));
        headers.push(("Connection".into(), "close".into()));
    }
    let mut upstream = TcpStream::connect((target.connect_host(), target.port)).await.map_err(|e| format!("连不上 dev server：{e}"))?;
    // 小段的流式输出（一条 SSE 事件）不等着凑包。
    let _ = client.set_nodelay(true);
    let _ = upstream.set_nodelay(true);
    upstream.write_all(&join_head(&first, &headers)).await.map_err(|e| e.to_string())?;
    if upgrade {
        upstream.write_all(&early).await.map_err(|e| e.to_string())?;
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        return Ok(());
    }
    let result = {
        let (mut from_client, mut to_client) = client.split();
        let (mut from_upstream, mut to_upstream) = upstream.split();
        // 浏览器这头发什么转什么（POST 的正文边读边转），一直读到它断开：它一走这个请求就算完，上游的连接跟着关，
        // 不停的流式回应不会留一个任务空转。
        let request = async {
            let mut alive = to_upstream.write_all(&early).await.is_ok();
            let mut buf = vec![0u8; PIECE];
            loop {
                match from_client.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    // 上游不收了（它可能已经回完）也接着读，等的是浏览器断开。
                    Ok(n) => alive = alive && to_upstream.write_all(&buf[..n]).await.is_ok(),
                }
            }
        };
        tokio::select! {
            _ = request => Ok(()),
            done = respond(&mut from_upstream, &mut to_client, is_head) => done,
        }
    };
    let _ = client.shutdown().await;
    result
}

/// 右栏 PREVIEW：给这个本机地址起代理，返回面板要开的地址。
#[tauri::command]
pub async fn preview_start(url: String) -> Result<String, String> {
    start(&url).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::{mpsc, Notify};
    use tokio::time::timeout;

    const WAIT: Duration = Duration::from_secs(5);

    /// 给假的 dev server 起代理，返回代理的端口。
    async fn proxy_for(port: u16) -> u16 {
        let url = start(&format!("http://localhost:{port}/")).await.unwrap();
        url.trim_start_matches("http://127.0.0.1:").trim_end_matches('/').parse().unwrap()
    }

    /// 连上代理、把请求发出去（不关写的那一半：浏览器也不关）。
    async fn send(proxy: u16, request: &str) -> TcpStream {
        let mut c = TcpStream::connect(("127.0.0.1", proxy)).await.unwrap();
        c.write_all(request.replace("{proxy}", &proxy.to_string()).as_bytes()).await.unwrap();
        c
    }

    /// 读到出现 `needle` 为止；等太久、连接先断了都算失败。
    async fn read_until(stream: &mut TcpStream, seen: &mut Vec<u8>, needle: &str) {
        let mut chunk = [0u8; 1024];
        while find(seen, needle.as_bytes()).is_none() {
            let n = timeout(WAIT, stream.read(&mut chunk)).await.unwrap_or_else(|_| panic!("没等到 {needle:?}：{}", String::from_utf8_lossy(seen))).unwrap();
            assert!(n > 0, "没等到 {needle:?} 连接就断了：{}", String::from_utf8_lossy(seen));
            seen.extend_from_slice(&chunk[..n]);
        }
    }

    /// 读到代理关连接为止。
    async fn read_all(stream: &mut TcpStream, seen: &mut Vec<u8>) -> String {
        timeout(WAIT, stream.read_to_end(seen)).await.expect("代理没有结束这个回应").unwrap();
        String::from_utf8(seen.clone()).unwrap()
    }

    /// 等对面断开（读到结束或者出错）。
    async fn until_closed(stream: &mut TcpStream) {
        let mut chunk = [0u8; 1024];
        while matches!(stream.read(&mut chunk).await, Ok(n) if n > 0) {}
    }

    #[test]
    fn only_local_http_targets() {
        assert_eq!(Target::parse("http://localhost:5173/docs?x=1").unwrap(), Target { host: "localhost".into(), port: 5173, path: "/docs?x=1".into() });
        assert_eq!(Target::parse("http://127.0.0.1:3000").unwrap().path, "/");
        assert_eq!(Target::parse("http://0.0.0.0:8080/").unwrap().host, "127.0.0.1");
        assert!(Target::parse("https://localhost:5173").is_err());
        assert!(Target::parse("http://example.com:80/").is_err());
        // 分块的正文：一个字节一个字节喂也解得对，结尾之后的不吃。
        let wire = b"5\r\nhello\r\n6;x=1\r\n world\r\n0\r\nX-Trailer: 1\r\n\r\nNEXT";
        let (mut chunks, mut body) = (Chunked::default(), Vec::new());
        assert_eq!(chunks.feed(wire, Some(&mut body)), wire.len() - 4);
        assert!(chunks.done() && body == b"hello world");
        let (mut chunks, mut body) = (Chunked::default(), Vec::new());
        let used: usize = wire.iter().map(|b| chunks.feed(&[*b], Some(&mut body))).sum();
        assert!(used == wire.len() - 4 && chunks.done() && body == b"hello world");
        let out = String::from_utf8(inject(b"<html><HEAD><title>x</title></HEAD><body>hi</body></html>")).unwrap();
        assert!(out.find("data-peck").unwrap() < out.find("</HEAD>").unwrap());
        // 没有 </head>：<body 前面；都没有：最前，但在 doctype 后面。
        assert!(inject(b"<html><BODY class=a>hi").starts_with(b"<html><script data-peck"));
        assert!(inject(b"<p>hi</p>").starts_with(b"<script data-peck"));
        assert!(inject(b"\n<!-- a --> <!DOCTYPE html>\n<p>hi</p>").starts_with(b"\n<!-- a --> <!DOCTYPE html><script data-peck"));
        // 边转边插：标记被切成两段也只插一次，插完后面的原样过。
        let mut injector = Injector::default();
        assert!(injector.push(b"<html><head><title>x</title></he").is_empty() && injector.waiting());
        let mut out = injector.push(b"ad><body>");
        out.extend(injector.push(b"hi</body>"));
        out.extend(injector.flush());
        assert_eq!(out, inject(b"<html><head><title>x</title></head><body>hi</body>"));
    }

    #[tokio::test]
    async fn html_gets_the_script_and_assets_pass_through() {
        // 一个假的 dev server：/ 回分块的 HTML（带 CSP），/a.js 原样。
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = server.accept().await {
                tokio::spawn(async move {
                    let (head, _) = read_head(&mut s).await.unwrap();
                    let (first, headers) = split_head(&head);
                    assert_eq!(header(&headers, "host"), Some(format!("localhost:{port}").as_str()));
                    assert!(header(&headers, "accept-encoding").is_none());
                    let reply: Vec<u8> = if first.starts_with("GET / ") {
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'self'\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n<html>\r\nf\r\n<head></head>hi\r\n7\r\n</html>\r\n0\r\n\r\n".to_vec()
                    } else {
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: 9\r\n\r\nlet a=1;\n".to_vec()
                    };
                    s.write_all(&reply).await.unwrap();
                    s.shutdown().await.unwrap();
                });
            }
        });
        let url = start(&format!("http://localhost:{port}/")).await.unwrap();
        assert!(url.starts_with("http://127.0.0.1:") && url.ends_with('/'));
        // 同一个 dev server 再要一次：还是那个代理。
        assert_eq!(start(&format!("http://localhost:{port}/x")).await.unwrap(), format!("{}x", url));
        let proxy_port: u16 = url.trim_start_matches("http://127.0.0.1:").trim_end_matches('/').parse().unwrap();
        let get = |path: &'static str| async move {
            let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
            c.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{proxy_port}\r\nAccept-Encoding: gzip\r\n\r\n").as_bytes()).await.unwrap();
            let mut out = Vec::new();
            c.read_to_end(&mut out).await.unwrap();
            String::from_utf8(out).unwrap()
        };
        let page = get("/").await;
        let (head, body) = page.split_once("\r\n\r\n").unwrap();
        assert!(body.starts_with("<html><head><script data-peck=\"preview\">") && body.ends_with("</head>hi</html>"), "{body}");
        // 分块的 HTML 解开了发：不带长度，以断开连接作结束。
        assert!(head.contains("Connection: close") && !head.to_lowercase().contains("content-length"));
        assert!(!head.to_lowercase().contains("content-security-policy") && !head.to_lowercase().contains("chunked"));
        let js = get("/a.js").await;
        assert!(js.ends_with("\r\n\r\nlet a=1;\n"));
        // 别的 Host（DNS rebinding）不转。
        let mut c = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        c.write_all(format!("GET / HTTP/1.1\r\nHost: evil.example:{proxy_port}\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = Vec::new();
        c.read_to_end(&mut out).await.unwrap();
        assert!(String::from_utf8(out).unwrap().starts_with("HTTP/1.1 403"));
    }

    #[tokio::test]
    async fn streams_arrive_piece_by_piece_and_do_not_block_other_requests() {
        // 假的 dev server：/events 是不会结束的 SSE（先发一条，放行了再发第二条，自己从不断开）；/a.js 给了长度、也不断开。
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        let gate = Arc::new(Notify::new());
        let (gone, mut gone_rx) = mpsc::unbounded_channel::<&'static str>();
        let server_gate = gate.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = server.accept().await {
                let (gate, gone) = (server_gate.clone(), gone.clone());
                tokio::spawn(async move {
                    let (head, _) = read_head(&mut s).await.unwrap();
                    let (first, _) = split_head(&head);
                    if first.starts_with("GET /events ") {
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\ndata: one\n\n").await.unwrap();
                        gate.notified().await;
                        s.write_all(b"data: two\n\n").await.unwrap();
                        until_closed(&mut s).await;
                        let _ = gone.send("events");
                    } else {
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: 9\r\n\r\nlet a=1;\n").await.unwrap();
                        until_closed(&mut s).await;
                    }
                });
            }
        });
        let proxy = proxy_for(port).await;
        let mut events = send(proxy, "GET /events HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\nAccept: text/event-stream\r\n\r\n").await;
        // 第一条在第二条发出来之前就到了：没有攒着等上游结束。
        let mut seen = Vec::new();
        read_until(&mut events, &mut seen, "data: one\n\n").await;
        let text = String::from_utf8_lossy(&seen).to_string();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n") && text.contains("Content-Type: text/event-stream") && text.contains("Connection: close"), "{text}");
        assert!(!text.contains("data: two"));
        assert!(timeout(Duration::from_millis(200), events.read(&mut [0u8; 64])).await.is_err(), "第二条还没放行，不该有东西来");
        // 流还开着，别的请求照样办完（上游没断开，靠声明的长度结束）。
        let mut asset = send(proxy, "GET /a.js HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\n\r\n").await;
        let js = read_all(&mut asset, &mut Vec::new()).await;
        assert!(js.contains("Content-Length: 9\r\n") && js.ends_with("\r\n\r\nlet a=1;\n"), "{js}");
        gate.notify_one();
        read_until(&mut events, &mut seen, "data: two\n\n").await;
        // 浏览器关掉这条流：上游的连接跟着关。
        drop(events);
        assert_eq!(timeout(WAIT, gone_rx.recv()).await.expect("浏览器走了，上游的连接没关"), Some("events"));
    }

    #[tokio::test]
    async fn html_gets_the_script_with_a_length_chunked_or_trickling() {
        const PAGE: &str = "<!doctype html><html><head><title>t</title></head><body>hi</body></html>";
        // 假的 dev server，三种 HTML，都不主动断开：/len 给长度；/stream 分块、先出页头，放行了再出后半；/drip 没有 head、body，出一点就停住。
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        let gate = Arc::new(Notify::new());
        let server_gate = gate.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = server.accept().await {
                let gate = server_gate.clone();
                tokio::spawn(async move {
                    let (head, _) = read_head(&mut s).await.unwrap();
                    let (first, _) = split_head(&head);
                    if first.starts_with("GET /len ") {
                        s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nX-Frame-Options: DENY\r\nContent-Length: {}\r\n\r\n{PAGE}", PAGE.len()).as_bytes()).await.unwrap();
                    } else if first.starts_with("GET /stream ") {
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'self'\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n<html>\r\n1d\r\n<head></head><body><p>one</p").await.unwrap();
                        s.write_all(b">\r\n").await.unwrap();
                        gate.notified().await;
                        s.write_all(b"18\r\n<p>two</p></body></html>\r\n0\r\n\r\n").await.unwrap();
                    } else {
                        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<p>tick</p>").await.unwrap();
                    }
                    until_closed(&mut s).await;
                });
            }
        });
        let proxy = proxy_for(port).await;

        // 给了长度的：长度加上脚本的，到长度就结束。
        let mut c = send(proxy, "GET /len HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\n\r\n").await;
        let page = read_all(&mut c, &mut Vec::new()).await;
        let (head, body) = page.split_once("\r\n\r\n").unwrap();
        assert_eq!(body.as_bytes(), inject(PAGE.as_bytes()));
        assert!(body.find("<script data-peck=\"preview\">").unwrap() > body.find("</title>").unwrap());
        assert!(head.contains(&format!("Content-Length: {}", body.len())), "{head}");
        assert!(!head.to_lowercase().contains("x-frame-options"));

        // 分块的流式 HTML：页头一到就带着脚本出来，不等后半；全部到齐后是解开的整页，脚本只有一份。
        let mut c = send(proxy, "GET /stream HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\n\r\n").await;
        let mut seen = Vec::new();
        read_until(&mut c, &mut seen, "<p>one</p>").await;
        let text = String::from_utf8_lossy(&seen).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        assert!(body.starts_with("<html><head><script data-peck=\"preview\">") && body.ends_with("</head><body><p>one</p>"), "{body}");
        let head = head.to_lowercase();
        assert!(!head.contains("content-security-policy") && !head.contains("chunked") && !head.contains("content-length") && head.contains("connection: close"), "{head}");
        gate.notify_one();
        let page = read_all(&mut c, &mut seen).await;
        assert!(page.ends_with("</head><body><p>one</p><p>two</p></body></html>"), "{page}");
        assert_eq!(page.matches("<script data-peck=\"preview\">").count(), 1);

        // 没有 head、body 又一点点出的：等一小会儿就放行，脚本在最前。
        let mut c = send(proxy, "GET /drip HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\n\r\n").await;
        let mut seen = Vec::new();
        read_until(&mut c, &mut seen, "<p>tick</p>").await;
        let text = String::from_utf8_lossy(&seen).to_string();
        assert!(text.split_once("\r\n\r\n").unwrap().1.starts_with("<script data-peck=\"preview\">"), "{text}");
    }

    #[tokio::test]
    async fn request_bodies_reach_the_dev_server() {
        // 假的 dev server：把收到的请求正文（按长度的、分块的）原样回出来。
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = server.accept().await {
                tokio::spawn(async move {
                    let (head, early) = read_head(&mut s).await.unwrap();
                    let (first, headers) = split_head(&head);
                    assert!(first.starts_with("POST /echo "));
                    let mut body = Vec::new();
                    if let Some(length) = header(&headers, "content-length").and_then(|v| v.parse::<usize>().ok()) {
                        body = early;
                        let mut rest = vec![0u8; length - body.len()];
                        s.read_exact(&mut rest).await.unwrap();
                        body.extend(rest);
                    } else {
                        assert_eq!(header(&headers, "transfer-encoding"), Some("chunked"));
                        let mut chunks = Chunked::default();
                        chunks.feed(&early, Some(&mut body));
                        let mut chunk = [0u8; 1024];
                        while !chunks.done() {
                            let n = s.read(&mut chunk).await.unwrap();
                            assert!(n > 0, "请求正文没传完");
                            chunks.feed(&chunk[..n], Some(&mut body));
                        }
                    }
                    s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                    s.write_all(&body).await.unwrap();
                    s.shutdown().await.unwrap();
                });
            }
        });
        let proxy = proxy_for(port).await;
        // 按长度的：正文在请求头之后分两次到。
        let mut c = send(proxy, "POST /echo HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\nContent-Type: application/json\r\nContent-Length: 17\r\n\r\n").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        c.write_all(b"{\"name\":").await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        c.write_all(b"\"peck\"}\n\n").await.unwrap();
        let reply = read_all(&mut c, &mut Vec::new()).await;
        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n") && reply.ends_with("\r\n\r\n{\"name\":\"peck\"}\n\n"), "{reply}");
        // 分块的。
        let mut c = send(proxy, "POST /echo HTTP/1.1\r\nHost: 127.0.0.1:{proxy}\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n").await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        c.write_all(b"6\r\n world\r\n0\r\n\r\n").await.unwrap();
        let reply = read_all(&mut c, &mut Vec::new()).await;
        assert!(reply.ends_with("\r\n\r\nhello world"), "{reply}");
    }
}
