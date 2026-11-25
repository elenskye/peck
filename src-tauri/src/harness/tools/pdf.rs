//! 读 PDF（v1.1.4）：两个工具，模型看说明自己挑。
//!
//! - `read_pdf`：macOS 自带的 PDFKit（经 `osascript -l JavaScript`，和截图一样不加依赖），在本机一页页抽文字，不花钱、
//!   马上出。没有文字层的页（扫描件）给能看图的模型附这一页的图（JPEG，最长边 1568），一次几张按窗口算（[`image_budget`]）。
//! - `ocr_pdf`：Mathpix（设置 → PROVIDERS → TOOL KEYS 填了 Key 才给，`gateway/mathpix.rs`）：扫描件、公式（出 LaTeX）、表格；
//!   按页收费，PDF 会传到 Mathpix，读完 Peck 把它那边的删掉。
//!
//! 两个都只读、哪一档都有；页码 `pages` 写 `3`、`1-5`、`1,3-4`，一次最多 [`MAX_PAGES`] 页（默认从第 1 页起）。

use super::{str_arg, ToolContext};
use crate::gateway::ImageInput;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 一次最多读几页。
pub const MAX_PAGES: usize = 20;
/// 不知道窗口多大时一次最多附几张页面图。
const DEFAULT_IMAGES: usize = 6;
/// 窗口每这么多 token 给一张页面图（一张约 1600 token，合起来约占窗口的 3%）。
const TOKENS_PER_IMAGE: u64 = 50_000;
/// 一页少于这么多个字就当没有文字层。
const TEXT_MIN: usize = 20;
/// 给模型的文字最多多长（超了停在那一页，告诉它从哪接着读）。
const TEXT_BYTES: usize = 100 * 1024;

/// 一次最多附几张页面图（v1.1.12）：按模型的上下文窗口，每 5 万 token 一张，至少 2 张、最多 [`MAX_PAGES`] 张——
/// 100 万的一次 20 页给满 20 张，20 万的 4 张；不知道窗口的 6 张。
pub fn image_budget(window: Option<u64>) -> usize {
    match window {
        Some(w) => usize::try_from(w / TOKENS_PER_IMAGE).unwrap_or(MAX_PAGES).clamp(2, MAX_PAGES),
        None => DEFAULT_IMAGES,
    }
}

/// `pages` → 页码（从 1 起，按顺序、不重复）。空的是从第 1 页起最多 `max` 页。
pub fn parse_pages(spec: Option<&str>, total: u32, max: usize) -> Result<Vec<u32>, String> {
    if total == 0 {
        return Err("这份 PDF 一页都没有。".into());
    }
    let spec = spec.map(str::trim).filter(|s| !s.is_empty());
    let Some(spec) = spec else {
        return Ok((1..=total.min(max as u32)).collect());
    };
    let mut pages = Vec::new();
    for part in spec.split([',', '，']).map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let a: u32 = a.parse().map_err(|_| format!("页码看不懂：{part}（写 3、1-5、1,3-4）"))?;
        let b: u32 = if b.is_empty() { total } else { b.parse().map_err(|_| format!("页码看不懂：{part}（写 3、1-5、1,3-4）"))? };
        if a == 0 || a > b {
            return Err(format!("页码看不懂：{part}（从 1 起，前面的不能比后面的大）"));
        }
        if a > total {
            return Err(format!("这份 PDF 只有 {total} 页，没有第 {a} 页。"));
        }
        pages.extend(a..=b.min(total));
    }
    pages.sort_unstable();
    pages.dedup();
    if pages.len() > max {
        return Err(format!("一次最多读 {max} 页（这次要了 {} 页），分几次读。", pages.len()));
    }
    Ok(pages)
}

/// 一页读出来的。
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub number: u32,
    pub text: String,
    /// 渲染出来的页面图（没有文字层、要了图的才有）。
    pub image: Option<PathBuf>,
}

/// 模型给的路径 → 项目里（或其他文件夹里）的一个 PDF。
pub fn resolve_pdf(ctx: &ToolContext, input: &Value) -> Result<PathBuf, String> {
    let raw = str_arg(input, "path").ok_or("缺 path。")?;
    let file = ctx.resolve_path(raw, false)?;
    if file.is_dir() {
        return Err(format!("{} 是文件夹。", ctx.rel(&file)));
    }
    if !file.is_file() {
        return Err(format!("读不到 {}（不存在或没权限）。", ctx.rel(&file)));
    }
    let mut head = [0u8; 5];
    let ok = std::fs::File::open(&file).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok();
    if !ok || &head != b"%PDF-" {
        return Err(format!("{} 不是 PDF（开头不是 %PDF-）。文本文件用 read_file 读。", ctx.rel(&file)));
    }
    Ok(file)
}

/// 交给 PDFKit 的脚本：`count <文件>` 给页数；`read <文件> <页码,…> <出图的文件夹或 ->` 给每页的文字（要出图的页渲染成 JPEG）；
/// `split <文件> <页码,…> <新文件>` 把这几页拆成一份新的。
const JXA: &str = r#"ObjC.import('PDFKit');
ObjC.import('AppKit');
function run(argv) {
  var doc = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath(argv[1]));
  if (doc.isNil()) return JSON.stringify({error: 'open'});
  if (doc.isLocked) return JSON.stringify({error: 'locked'});
  var total = Number(doc.pageCount);
  if (argv[0] === 'count') return JSON.stringify({total: total});
  if (argv[0] === 'split') {
    var keep = argv[2].split(',').map(function (x) { return parseInt(x, 10); });
    var part = $.PDFDocument.alloc.init;
    for (var j = 0; j < keep.length; j++) {
      if (keep[j] >= 1 && keep[j] <= total) part.insertPageAtIndex(doc.pageAtIndex(keep[j] - 1).copy, Number(part.pageCount));
    }
    return JSON.stringify({total: total, written: Number(part.pageCount), ok: part.writeToFile(argv[3])});
  }
  var wanted = argv[2].split(',').map(function (x) { return parseInt(x, 10); });
  var dir = argv[3], max = parseInt(argv[4], 10), min = parseInt(argv[5], 10), made = 0, out = [];
  for (var k = 0; k < wanted.length; k++) {
    var n = wanted[k];
    if (n < 1 || n > total) continue;
    var page = doc.pageAtIndex(n - 1);
    var s = page.string;
    var text = s.isNil() ? '' : s.js;
    var entry = {page: n, text: text};
    if (dir !== '-' && text.replace(/\s/g, '').length < min && made < max) {
      var b = page.boundsForBox(0);
      var scale = 1568 / Math.max(b.size.width, b.size.height, 1);
      var img = page.thumbnailOfSizeForBox({width: b.size.width * scale, height: b.size.height * scale}, 0);
      var rep = $.NSBitmapImageRep.imageRepWithData(img.TIFFRepresentation);
      var data = rep.representationUsingTypeProperties(3, $({NSImageCompressionFactor: 0.8}));
      var file = dir + '/page-' + n + '.jpg';
      if (data.writeToFileAtomically(file, true)) { entry.image = file; made++; }
    }
    out.push(entry);
  }
  return JSON.stringify({total: total, pages: out});
}"#;

fn pdfkit(args: &[&str]) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/osascript")
            .args(["-l", "JavaScript", "-e", JXA])
            .args(args)
            .output()
            .map_err(|e| format!("PDFKit 起不来：{e}"))?;
        if !out.status.success() {
            return Err(format!("PDFKit 读不了这份 PDF：{}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let v: Value = serde_json::from_slice(&out.stdout).map_err(|_| "PDFKit 的结果看不懂。".to_string())?;
        match v.get("error").and_then(Value::as_str) {
            Some("locked") => Err("这份 PDF 加了密码，读不了。".into()),
            Some(_) => Err("PDFKit 打不开这份 PDF（可能坏了）。".into()),
            None => Ok(v),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (args, JXA);
        Err("只有 Mac 上能读 PDF。".into())
    }
}

/// 页数。
pub fn page_count(file: &Path) -> Result<u32, String> {
    let v = pdfkit(&["count", &file.to_string_lossy()])?;
    Ok(v["total"].as_u64().unwrap_or(0) as u32)
}

/// 把这几页拆成一份新的 PDF（送 Mathpix 的只有这几页）。给回写了几页。
pub fn split(file: &Path, pages: &[u32], to: &Path) -> Result<u32, String> {
    let list: Vec<String> = pages.iter().map(u32::to_string).collect();
    let v = pdfkit(&["split", &file.to_string_lossy(), &list.join(","), &to.to_string_lossy()])?;
    if v["ok"] != Value::Bool(true) {
        return Err("拆不出这几页。".into());
    }
    Ok(v["written"].as_u64().unwrap_or(0) as u32)
}

/// 读这几页；`render` 给了就把没有文字层的页渲染到这个文件夹（最多 `max_images` 张）。
pub fn read_pages(file: &Path, pages: &[u32], render: Option<&Path>, max_images: usize) -> Result<Vec<Page>, String> {
    let list: Vec<String> = pages.iter().map(u32::to_string).collect();
    let dir = render.map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| "-".into());
    let v = pdfkit(&["read", &file.to_string_lossy(), &list.join(","), &dir, &max_images.to_string(), &TEXT_MIN.to_string()])?;
    Ok(v["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| Page {
            number: p["page"].as_u64().unwrap_or(0) as u32,
            text: p["text"].as_str().unwrap_or("").to_string(),
            image: p["image"].as_str().map(PathBuf::from),
        })
        .collect())
}

/// 一页有没有文字层。
pub fn has_text(text: &str) -> bool {
    text.chars().filter(|c| !c.is_whitespace()).count() >= TEXT_MIN
}

/// `read_pdf`：给模型的文字和附的页面图。`images` 是这次最多附几张（模型看不了图是 0，见 [`image_budget`]）；
/// `ocr` 是这个任务有没有 `ocr_pdf`。
pub fn read_pdf(ctx: &ToolContext, input: &Value, images: usize, ocr: bool) -> Result<(String, Vec<ImageInput>), String> {
    let file = resolve_pdf(ctx, input)?;
    let rel = ctx.rel(&file);
    let total = page_count(&file)?;
    let pages = parse_pages(str_arg(input, "pages"), total, MAX_PAGES)?;
    let vision = images > 0;
    let render = if vision {
        let n = ctx.spill_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let dir = ctx.spill_dir.join(format!("{n}-pdf"));
        std::fs::create_dir_all(&dir).ok().map(|_| dir)
    } else {
        None
    };
    let read = read_pages(&file, &pages, render.as_deref(), images)?;
    let (text, images) = format_pages(&rel, total, &read, vision, ocr);
    if let Some(dir) = &render {
        // 没出图的话空文件夹不留着。
        let _ = std::fs::remove_dir(dir);
    }
    Ok((text, images))
}

/// 拼给模型看的：头一行写共几页、这次读了哪几页；每页一段；没文字层的页写明，附了图的说在后面。
pub fn format_pages(rel: &str, total: u32, pages: &[Page], vision: bool, ocr: bool) -> (String, Vec<ImageInput>) {
    let first = pages.first().map(|p| p.number).unwrap_or(1);
    let mut out = String::new();
    let mut images = Vec::new();
    let mut last = first;
    let mut cut = false;
    for p in pages {
        let body = if has_text(&p.text) {
            p.text.trim().to_string()
        } else {
            let attached = p.image.as_ref().and_then(|path| std::fs::read(path).ok());
            match attached {
                Some(bytes) => {
                    images.push(ImageInput { media_type: "image/jpeg".into(), data: crate::files::base64(&bytes) });
                    format!("（这一页没有文字层，可能是扫描件：附了这一页的图，第 {} 张）", images.len())
                }
                None if ocr => "（这一页没有文字层，可能是扫描件：要文字用 ocr_pdf 识别这一页）".into(),
                None if vision => "（这一页没有文字层，可能是扫描件；这次附的图满了，单独读这一页能拿到图）".into(),
                None => "（这一页没有文字层，可能是扫描件，读不出字）".into(),
            }
        };
        let block = format!("\n--- 第 {} 页 ---\n{body}\n", p.number);
        if out.len() + block.len() > TEXT_BYTES && !out.is_empty() {
            cut = true;
            break;
        }
        out.push_str(&block);
        last = p.number;
    }
    let shown = if first == last { format!("第 {first} 页") } else { format!("第 {first}–{last} 页") };
    let mut head = format!("{rel}：共 {total} 页，这次读了{shown}（PDFKit 在本机抽的文字，版式、公式、表格可能乱）。");
    if !images.is_empty() {
        head.push_str(&format!("附了 {} 张页面图，在结果后面。", images.len()));
    }
    let mut text = format!("{head}\n{out}");
    if cut {
        text.push_str(&format!("\n（超过 100KB，停在第 {last} 页；下次从第 {} 页读）", last + 1));
    } else if last < total && pages.last().map(|p| p.number) == Some(last) && pages.len() == MAX_PAGES {
        text.push_str(&format!("\n（后面还有，下次从第 {} 页读）", last + 1));
    }
    (text, images)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges() {
        assert_eq!(parse_pages(None, 3, 20).unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_pages(None, 50, 20).unwrap().len(), 20);
        assert_eq!(parse_pages(Some("2"), 5, 20).unwrap(), vec![2]);
        assert_eq!(parse_pages(Some("4-"), 5, 20).unwrap(), vec![4, 5]);
        assert_eq!(parse_pages(Some("1, 3-4，3"), 5, 20).unwrap(), vec![1, 3, 4]);
        assert_eq!(parse_pages(Some("2-9"), 5, 20).unwrap(), vec![2, 3, 4, 5], "超出的截到最后一页");
        assert!(parse_pages(Some("6"), 5, 20).unwrap_err().contains("只有 5 页"));
        assert!(parse_pages(Some("1-30"), 40, 20).unwrap_err().contains("最多读 20 页"));
        assert!(parse_pages(Some("x"), 5, 20).is_err() && parse_pages(Some("3-1"), 5, 20).is_err() && parse_pages(Some("0"), 5, 20).is_err());
    }

    #[test]
    fn image_budget_follows_the_window() {
        assert_eq!(image_budget(Some(1_000_000)), 20, "100 万的一次给满 20 张");
        assert_eq!(image_budget(Some(1_048_576)), 20, "多出来的不超过一次读的页数");
        assert_eq!(image_budget(Some(400_000)), 8);
        assert_eq!(image_budget(Some(200_000)), 4);
        assert_eq!(image_budget(Some(128_000)), 2);
        assert_eq!(image_budget(Some(32_000)), 2, "再小也给 2 张");
        assert_eq!(image_budget(None), 6, "不知道窗口的照旧 6 张");
    }

    #[test]
    fn pages_without_text_say_so_and_carry_images() {
        let dir = std::env::temp_dir().join(format!("peck-pdf-fmt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p2.jpg"), [0xFF, 0xD8, 0xFF]).unwrap();
        let pages = vec![
            Page { number: 1, text: "第一页的正文，足够长的一段文字在这里。".repeat(2), image: None },
            Page { number: 2, text: " ".into(), image: Some(dir.join("p2.jpg")) },
            Page { number: 3, text: "".into(), image: None },
        ];
        let (text, images) = format_pages("doc.pdf", 9, &pages, true, true);
        assert!(text.starts_with("doc.pdf：共 9 页，这次读了第 1–3 页"), "{text}");
        assert!(text.contains("--- 第 2 页 ---\n（这一页没有文字层") && text.contains("第 1 张"));
        assert!(text.contains("用 ocr_pdf 识别"), "图满了、有 Mathpix 时提 ocr_pdf");
        assert_eq!(images.len(), 1);
        let (text, images) = format_pages("doc.pdf", 3, &pages[2..], false, false);
        assert!(text.contains("读不出字") && images.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 真用 PDFKit 读一份系统打印出来的 PDF（只在 Mac 上）。
    #[cfg(target_os = "macos")]
    #[test]
    fn pdfkit_reads_text_and_renders_empty_pages() {
        let dir = std::env::temp_dir().join(format!("peck-pdf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "Hello PDF page one, with enough words to count as text.\n").unwrap();
        let out = std::process::Command::new("/usr/sbin/cupsfilter").arg(dir.join("a.txt")).output().unwrap();
        if !out.status.success() || !out.stdout.starts_with(b"%PDF-") {
            eprintln!("cupsfilter 出不了 PDF，跳过");
            return;
        }
        let file = dir.join("a.pdf");
        std::fs::write(&file, &out.stdout).unwrap();
        assert_eq!(page_count(&file).unwrap(), 1);
        let pages = read_pages(&file, &[1], Some(&dir), 20).unwrap();
        assert!(pages[0].text.contains("Hello PDF page one"), "{pages:?}");
        assert!(pages[0].image.is_none(), "有文字的页不出图");
        assert_eq!(split(&file, &[1], &dir.join("part.pdf")).unwrap(), 1);
        assert_eq!(page_count(&dir.join("part.pdf")).unwrap(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
