//! 内置预览面板（v0.8 Phase 12）的本机代理：右栏 PREVIEW 不直接开 dev server 的地址，而是开 Peck 在
//! `127.0.0.1` 上起的一个小代理。代理把请求原样转给 dev server（只认本机地址），回来的 HTML 里插一段小脚本
//! （`preview.js`：点元素写批注），别的原样回去；WebSocket（Vite 的热更新这类）升级以后两头直接对接。
//!
//! 为了能插脚本：请求去掉 `Accept-Encoding`（回来的不压缩）、改成 `Connection: close`（一次一个请求，读到
//! 结束就是整份）；HTML 去掉 `Content-Security-Policy`、`X-Frame-Options`（不然放不进面板、脚本跑不了），
//! 分块传输的先拼成整份再算长度。只为开发时自己看，不做缓存、不做 HTTPS。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const SCRIPT: &str = include_str!("preview.js");
/// 一次回应最多读多少（dev server 的单个文件不会更大）。
const MAX_RESPONSE: usize = 64 * 1024 * 1024;
const MAX_HEAD: usize = 64 * 1024;

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
        while let Ok((client, _)) = listener.accept().await {
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

/// 读到请求头结束；返回（头，头后面已经读到的字节）。
async fn read_head(stream: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            let rest = buf.split_off(i + 4);
            return Ok((buf, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err("请求头太长。".into());
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

/// 分块传输的正文拼成整份。
fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut at = 0;
    while at < body.len() {
        let Some(eol) = find(&body[at..], b"\r\n") else { break };
        let size_text = String::from_utf8_lossy(&body[at..at + eol]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        at += eol + 2;
        if size == 0 || at + size > body.len() {
            break;
        }
        out.extend_from_slice(&body[at..at + size]);
        at += size + 2;
    }
    out
}

/// 把脚本插进 HTML：`</head>` 前面，没有就 `<body` 前面，再没有就放最前。
pub fn inject(html: &[u8]) -> Vec<u8> {
    let tag = format!("<script data-peck=\"preview\">{SCRIPT}</script>");
    let lower = html.to_ascii_lowercase();
    let at = find(&lower, b"</head>").or_else(|| find(&lower, b"<body")).unwrap_or(0);
    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
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
    // Host 换成 dev server 自己的（Vite 这类会查）；Origin 只有是预览面板自己的才换，别的原样交给 dev server 判断。不压缩、一次一个请求。
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
    upstream.write_all(&join_head(&first, &headers)).await.map_err(|e| e.to_string())?;
    upstream.write_all(&early).await.map_err(|e| e.to_string())?;
    if upgrade {
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        return Ok(());
    }
    // 请求的正文（按 Content-Length）。
    let length: usize = header(&headers, "content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if length > early.len() {
        let mut rest = vec![0u8; length - early.len()];
        client.read_exact(&mut rest).await.map_err(|e| e.to_string())?;
        upstream.write_all(&rest).await.map_err(|e| e.to_string())?;
    }
    let mut response = Vec::new();
    (&mut upstream).take(MAX_RESPONSE as u64).read_to_end(&mut response).await.map_err(|e| e.to_string())?;
    let Some(split) = find(&response, b"\r\n\r\n") else {
        client.write_all(&response).await.map_err(|e| e.to_string())?;
        return Ok(());
    };
    let (status, mut rh) = split_head(&response[..split]);
    let mut body = response[split + 4..].to_vec();
    let html = header(&rh, "content-type").is_some_and(|t| t.to_ascii_lowercase().contains("text/html"));
    let ok = status.split_whitespace().nth(1).is_some_and(|c| c.starts_with('2'));
    if html && ok && !is_head {
        if header(&rh, "transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
            body = dechunk(&body);
        }
        body = inject(&body);
        rh.retain(|(k, _)| {
            let k = k.to_ascii_lowercase();
            !matches!(k.as_str(), "content-length" | "transfer-encoding" | "content-security-policy" | "content-security-policy-report-only" | "x-frame-options" | "connection")
        });
        rh.push(("Content-Length".into(), body.len().to_string()));
        rh.push(("Connection".into(), "close".into()));
        client.write_all(&join_head(&status, &rh)).await.map_err(|e| e.to_string())?;
        client.write_all(&body).await.map_err(|e| e.to_string())?;
    } else {
        client.write_all(&response).await.map_err(|e| e.to_string())?;
    }
    let _ = client.shutdown().await;
    Ok(())
}

/// 右栏 PREVIEW：给这个本机地址起代理，返回面板要开的地址。
#[tauri::command]
pub async fn preview_start(url: String) -> Result<String, String> {
    start(&url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_http_targets() {
        assert_eq!(Target::parse("http://localhost:5173/docs?x=1").unwrap(), Target { host: "localhost".into(), port: 5173, path: "/docs?x=1".into() });
        assert_eq!(Target::parse("http://127.0.0.1:3000").unwrap().path, "/");
        assert_eq!(Target::parse("http://0.0.0.0:8080/").unwrap().host, "127.0.0.1");
        assert!(Target::parse("https://localhost:5173").is_err());
        assert!(Target::parse("http://example.com:80/").is_err());
        assert_eq!(dechunk(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"), b"hello world");
        let out = String::from_utf8(inject(b"<html><HEAD><title>x</title></HEAD><body>hi</body></html>")).unwrap();
        assert!(out.find("data-peck").unwrap() < out.find("</HEAD>").unwrap());
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
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
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
}
