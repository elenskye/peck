//! Mathpix（v1.1.4）：Peck Loop 的 `ocr_pdf` 背后。扫描件、公式（出 LaTeX）、表格认得比本机的 PDFKit 好，按页收费。
//!
//! - 只送要读的那几页：先用 PDFKit 在本机拆成一份小 PDF（`harness/tools/pdf.rs`），再 `POST /v3/pdf` 上传
//!   （multipart：`file` + `options_json`）；每 2 秒查一次 `GET /v3/pdf/{id}`，`completed` 了取 `.mmd`（Mathpix Markdown：
//!   公式是 LaTeX，表格是 LaTeX 的 tabular），最后 `DELETE` 掉它那边的文件和结果（删不掉的在结果里写一句）。
//!   最多等 5 分钟，站主按停就不等了（也删）。
//! - 认证是两个请求头 `app_id`、`app_key`（设置 → PROVIDERS → PDF，存在钥匙串「其他」那一包）。错误大多是 HTTP 200 带
//!   `error_info.id`，所以每次都看 body。
//! - 费用：每页 $0.005（官网 v3/pdf 的价，100 万页以内），按送去的页数记，算进这个任务和花费上限。
//! - 模拟模式不联网，给一段假的结果、不记费用。

use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::sync::watch;
use zeroize::Zeroizing;

const BASE: &str = "https://api.mathpix.com/v3/pdf";
/// 每页多少美元。
pub const PRICE_PER_PAGE: f64 = 0.005;
const POLL: Duration = Duration::from_secs(2);
const WAIT: Duration = Duration::from_secs(300);

/// 谁来识别。
#[derive(Clone)]
pub enum Mathpix {
    Real { app_id: Zeroizing<String>, app_key: Zeroizing<String> },
    Mock,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "建不了 Mathpix 的连接。".to_string())
}

/// Mathpix 的错误 → 一句中文（不带 Key）。
fn error_text(status: u16, v: &Value) -> Option<String> {
    let id = v["error_info"]["id"].as_str();
    let message = v["error_info"]["message"].as_str().or_else(|| v["error"].as_str());
    if id.is_none() && message.is_none() && (200..300).contains(&status) {
        return None;
    }
    let what = match (status, id) {
        (401, _) | (_, Some("http_unauthorized")) => "app_id 或 app_key 不对，或者账户停用了".to_string(),
        (429, _) | (_, Some("http_max_requests")) => "请求太频繁，或这个月的页数额度用完了".to_string(),
        (_, Some("pdf_encrypted")) => "这份 PDF 加了密码".to_string(),
        (_, Some("pdf_page_limit_exceeded")) => "页数超过了 Mathpix 的上限".to_string(),
        (s, _) if s >= 500 => "Mathpix 那边出错了".to_string(),
        (s, None) if !(200..300).contains(&s) => format!("Mathpix 返回 {s}"),
        _ => "Mathpix 没识别成".to_string(),
    };
    Some(match message {
        Some(m) => format!("{what}（{}）", super::convert::short(m, 120)),
        None => what,
    })
}

async fn json_of(response: reqwest::Response) -> Result<(u16, Value), String> {
    let status = response.status().as_u16();
    let text = response.text().await.map_err(|_| "Mathpix 的回应读到一半断了。".to_string())?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

/// multipart 的 body（不用另加 reqwest 的 multipart 依赖）：`file` 一份 PDF、`options_json` 一段 JSON。
fn multipart(boundary: &str, pdf: &[u8], options: &Value) -> Vec<u8> {
    let mut body = Vec::with_capacity(pdf.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"options_json\"\r\n\r\n{options}\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"pages.pdf\"\r\nContent-Type: application/pdf\r\n\r\n").as_bytes());
    body.extend_from_slice(pdf);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// 送去识别时的选项：公式用 `$` / `$$` 包（模型最熟），留着 Mathpix 默认的去空格。
pub fn options() -> Value {
    json!({"math_inline_delimiters": ["$", "$"], "math_display_delimiters": ["$$", "$$"], "rm_spaces": true})
}

impl Mathpix {
    /// 识别一份（已经拆好、只有要读的几页的）PDF，给回 Mathpix Markdown 和「它那边删没删掉」（删不掉的原因）。
    pub async fn convert(&self, pdf: &Path, mut cancel: watch::Receiver<bool>) -> Result<(String, Option<String>), String> {
        let (app_id, app_key) = match self {
            Mathpix::Mock => return Ok(("（模拟模式：没有真的送 Mathpix，下面是假的结果）\n\n# 第一节\n\n公式 $E = mc^2$。\n\n\\begin{tabular}{|l|l|}\n\\hline a & b \\\\\n\\hline 1 & 2 \\\\\n\\hline\n\\end{tabular}".into(), None)),
            Mathpix::Real { app_id, app_key } => (app_id.as_str(), app_key.as_str()),
        };
        let bytes = std::fs::read(pdf).map_err(|e| format!("读不了拆出来的 PDF：{e}"))?;
        let boundary = format!("peck-{}", uuid::Uuid::new_v4().simple());
        let http = client()?;
        let sent = http
            .post(BASE)
            .header("app_id", app_id)
            .header("app_key", app_key)
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .body(multipart(&boundary, &bytes, &options()))
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "传给 Mathpix 超时了。".to_string() } else { "连不上 Mathpix。".to_string() })?;
        let (status, v) = json_of(sent).await?;
        if let Some(why) = error_text(status, &v) {
            return Err(why);
        }
        let id = v["pdf_id"].as_str().filter(|s| !s.is_empty()).ok_or("Mathpix 没给回 pdf_id。")?.to_string();
        let result = self.wait_and_fetch(&http, &id, &mut cancel).await;
        // 不管成没成都删掉它那边的（原件和结果），不留在别人的服务器上；删不掉的写一句给模型和站主看。
        let removed = match http.delete(format!("{BASE}/{id}")).header("app_id", app_id).header("app_key", app_key).send().await {
            Ok(r) if r.status().is_success() => None,
            Ok(r) => Some(format!("返回 {}", r.status().as_u16())),
            Err(_) => Some("连不上".to_string()),
        };
        result.map(|text| (text, removed))
    }

    async fn wait_and_fetch(&self, http: &reqwest::Client, id: &str, cancel: &mut watch::Receiver<bool>) -> Result<String, String> {
        let Mathpix::Real { app_id, app_key } = self else { return Err("模拟模式不查。".into()) };
        let started = std::time::Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(POLL) => {}
                _ = cancel.wait_for(|c| *c) => return Err("已停止。".into()),
            }
            let r = http.get(format!("{BASE}/{id}")).header("app_id", app_id.as_str()).header("app_key", app_key.as_str()).send().await.map_err(|_| "查 Mathpix 的进度时断了。".to_string())?;
            let (status, v) = json_of(r).await?;
            match v["status"].as_str() {
                Some("completed") => break,
                Some("error") => return Err(error_text(status, &v).unwrap_or_else(|| "Mathpix 没识别成。".into())),
                _ if status >= 400 && status != 404 => return Err(error_text(status, &v).unwrap_or_else(|| format!("Mathpix 返回 {status}"))),
                _ => {}
            }
            if started.elapsed() > WAIT {
                return Err("Mathpix 5 分钟还没识别完，先不等了。".into());
            }
        }
        let r = http.get(format!("{BASE}/{id}.mmd")).header("app_id", app_id.as_str()).header("app_key", app_key.as_str()).send().await.map_err(|_| "取 Mathpix 的结果时断了。".to_string())?;
        let status = r.status().as_u16();
        let text = r.text().await.map_err(|_| "Mathpix 的结果读到一半断了。".to_string())?;
        if !(200..300).contains(&status) {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Err(error_text(status, &v).unwrap_or_else(|| format!("Mathpix 返回 {status}")));
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_read_from_the_body_even_on_200() {
        assert_eq!(error_text(200, &json!({"pdf_id": "x"})), None);
        let bad = json!({"error": "Unauthorized", "error_info": {"id": "http_unauthorized", "message": "Unauthorized"}});
        assert!(error_text(401, &bad).unwrap().starts_with("app_id 或 app_key 不对"));
        let enc = json!({"error": "PDF is encrypted", "error_info": {"id": "pdf_encrypted", "message": "PDF is encrypted"}});
        assert!(error_text(200, &enc).unwrap().contains("加了密码"));
        assert!(error_text(503, &Value::Null).unwrap().contains("那边出错了"));
    }

    #[test]
    fn multipart_has_both_fields() {
        let body = String::from_utf8_lossy(&multipart("b1", b"%PDF-1.4 x", &options())).into_owned();
        assert!(body.starts_with("--b1\r\nContent-Disposition: form-data; name=\"options_json\"\r\n\r\n{"));
        assert!(body.contains("name=\"file\"; filename=\"pages.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF-1.4 x\r\n--b1--\r\n"));
        assert!(body.contains("\"math_inline_delimiters\":[\"$\",\"$\"]"));
    }

    #[tokio::test]
    async fn mock_does_not_go_online() {
        let (_tx, rx) = watch::channel(false);
        let (text, removed) = Mathpix::Mock.convert(Path::new("/nope.pdf"), rx).await.unwrap();
        assert!(text.contains("模拟模式") && removed.is_none());
    }

    /// 真识别一次（花钱，要手动跑）：`PECK_MATHPIX_ID=… PECK_MATHPIX_KEY=… PECK_LIVE_PDF=<一两页的 PDF> cargo test live_mathpix -- --ignored --nocapture`。
    #[tokio::test]
    #[ignore]
    async fn live_mathpix() {
        let m = Mathpix::Real { app_id: Zeroizing::new(std::env::var("PECK_MATHPIX_ID").expect("PECK_MATHPIX_ID")), app_key: Zeroizing::new(std::env::var("PECK_MATHPIX_KEY").expect("PECK_MATHPIX_KEY")) };
        let (_tx, rx) = watch::channel(false);
        let (text, removed) = m.convert(Path::new(&std::env::var("PECK_LIVE_PDF").expect("PECK_LIVE_PDF")), rx).await.unwrap();
        eprintln!("{text}\n删掉了吗：{}", removed.as_deref().unwrap_or("删掉了"));
        assert!(!text.trim().is_empty());
        assert_eq!(removed, None, "Mathpix 那边的要删掉");
    }
}
