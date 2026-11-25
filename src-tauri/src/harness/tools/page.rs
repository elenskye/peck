//! `screenshot_page`（v1.1.11）：截一个本机网页给模型看（自己做的界面、`?mock` 这类）。
//!
//! 2026-11-25 PLOOP 开发 Peck 时没办法看自己改的界面（截 App 窗口要屏幕录制权限），就起无头 Chrome：命令的 HOME 是
//! Peck 给的，Chrome 找不到钥匙串，系统连弹一串「Keychain Not Found」。这里改用 macOS 自带的 WebKit：经
//! `osascript -l JavaScript`（和 `read_pdf` 一样不加依赖）开一个放在屏幕外的无边框窗口截图（`page.js`），
//! 不抢焦点、不要屏幕录制权限、不碰钥匙串。只开本机地址（dev server）。

use super::{bool_arg, str_arg, u64_arg, ToolContext};
use crate::gateway::ImageInput;
use serde_json::Value;
use std::time::{Duration, Instant};

const JXA: &str = include_str!("page.js");

/// 能截的地址：本机的 http(s)。
fn local_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).ok_or("只截本机的网页（http://localhost:端口/…）。")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if authority.starts_with('[') { authority.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority).to_lowercase() };
    if !(matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") || host.ends_with(".localhost")) {
        return Err("只截本机的网页（localhost、127.0.0.1），外面的网站用 web_fetch 读。".into());
    }
    Ok(url.replacen("0.0.0.0", "127.0.0.1", 1))
}

/// 截。返回（给模型的一句，图）。
pub fn screenshot_page(ctx: &ToolContext, input: &Value) -> Result<(String, ImageInput), String> {
    let url = local_url(str_arg(input, "url").ok_or("缺 url。")?)?;
    let width = u64_arg(input, "width").unwrap_or(1360).clamp(320, 2560);
    let height = u64_arg(input, "height").unwrap_or(860).clamp(320, 2400);
    let look = match str_arg(input, "color_scheme").map(str::trim) {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "",
    };
    let still = if bool_arg(input, "animations") == Some(true) { "0" } else { "1" };
    let script = str_arg(input, "script").unwrap_or("").to_string();
    let wait = u64_arg(input, "wait_ms").unwrap_or(300).min(10_000);
    std::fs::create_dir_all(&ctx.spill_dir).map_err(|e| format!("存不了截图：{e}"))?;
    let n = ctx.spill_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let path = ctx.spill_dir.join(format!("{n}-page.png"));
    let out = run_jxa(&[&url, &path.to_string_lossy(), &width.to_string(), &height.to_string(), look, still, &script, &wait.to_string()])?;
    let v: Value = serde_json::from_str(out.trim()).map_err(|_| format!("截网页的结果看不懂：{}", super::clip_bytes(out.trim(), 300)))?;
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        return Err(format!("截不了 {url}：{e}"));
    }
    // 截出来是 2 倍的（Retina）：缩到长边 1568，和截窗口一样。
    let _ = std::process::Command::new("/usr/bin/sips").args(["-Z", "1568"]).arg(&path).output();
    let bytes = std::fs::read(&path).map_err(|e| format!("读不了截图：{e}"))?;
    let mut text = format!("截了 {}（{width}×{height}{}），图在下面。截图存在 {}。", v.get("url").and_then(Value::as_str).unwrap_or(&url), if look.is_empty() { String::new() } else { format!("，{look}") }, path.display());
    if let Some(r) = v.get("result").and_then(Value::as_str).filter(|r| !r.is_empty()) {
        text.push_str(&format!("\nscript 返回：{}", super::clip_bytes(r, 4000)));
    }
    Ok((text, ImageInput { media_type: "image/png".into(), data: crate::files::base64(&bytes) }))
}

#[cfg(target_os = "macos")]
fn run_jxa(args: &[&str]) -> Result<String, String> {
    let mut child = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", JXA])
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("截网页起不来：{e}"))?;
    // 加载 20 秒、脚本 30 秒、截图 10 秒，再留一点。
    let deadline = Instant::now() + Duration::from_secs(75);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("截网页超时了。".into());
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("截网页出错了：{}", super::clip_bytes(String::from_utf8_lossy(&out.stderr).trim(), 300)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn run_jxa(_args: &[&str]) -> Result<String, String> {
    let _ = JXA;
    Err("只有 Mac 上能截网页。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_pages() {
        for ok in ["http://localhost:1430/?mock", "http://127.0.0.1:5173", "https://localhost:8443/a", "http://[::1]:3000/", "http://app.localhost:3000/x", "http://0.0.0.0:8000/"] {
            assert!(local_url(ok).is_ok(), "{ok}");
        }
        assert_eq!(local_url("http://0.0.0.0:8000/").unwrap(), "http://127.0.0.1:8000/");
        for bad in ["https://example.com", "file:///etc/passwd", "localhost:1430", "http://localhost.evil.com/", "http://192.168.1.2:80"] {
            assert!(local_url(bad).is_err(), "{bad}");
        }
    }

    /// 真截一次：Peck 在 127.0.0.1 上起一个只回一页 HTML 的小服务，脚本读出页面上的字。
    #[test]
    #[cfg(target_os = "macos")]
    fn takes_a_real_screenshot_and_runs_the_script() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(4) {
                let mut s = stream;
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body = "<html><body style='background:#123'><h1 id=t>peck-page-probe</h1></body></html>";
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        let p = super::super::testing::TempProject::new();
        let ctx = p.ctx();
        let (text, image) = screenshot_page(&ctx, &serde_json::json!({"url": format!("http://127.0.0.1:{port}/"), "width": 400, "height": 300, "script": "return document.getElementById('t').textContent"})).unwrap();
        assert!(text.contains("script 返回：peck-page-probe") && text.contains("截图存在"), "{text}");
        assert!(image.data.len() > 100 && image.media_type == "image/png");
    }
}
