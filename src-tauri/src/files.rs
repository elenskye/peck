//! 右栏 FILES：只看不改。照忽略规则列项目里的文件，读一个文件给预览。
//! v0.7：图片、PDF 带上 data URL 直接显示；Word、Excel 这类交给 Quick Look（`qlmanage` 出一张缩略图，
//! 也能叫出系统的 Quick Look 窗口）。代码上色、md 和 html 切源码 / 预览在界面里做。

use serde::Serialize;
use std::fs;
use std::path::{Component, Path};

use crate::watch::GitignoreRules;

/// 列表最多给这么多个文件，再多就写「太多了」。
pub const MAX_ENTRIES: usize = 4000;
/// 超过这个大小不预览。
pub const MAX_PREVIEW_BYTES: u64 = 400 * 1024;
/// 图片、PDF 超过这个大小不预览（要转成 data URL 送给界面）。
pub const MAX_MEDIA_BYTES: u64 = 24 * 1024 * 1024;
/// 交给 Quick Look 的文件（Word、Excel、PowerPoint、iWork、RTF、OpenDocument）。
const OFFICE_EXTENSIONS: [&str; 13] = ["doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "pages", "numbers", "rtf", "odt", "ods", "odp"];

const IMAGE_EXTENSIONS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tiff", "heic",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileListing {
    pub entries: Vec<FileEntry>,
    /// 照忽略规则藏起来的文件和文件夹数（一个文件夹算一个）。
    pub hidden: usize,
    /// 文件太多，只列了前 `MAX_ENTRIES` 个。
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    pub path: String,
    /// `text`、`image`、`binary`、`large`、`missing`。只有 `text` 带内容。
    pub kind: String,
    pub content: Option<String>,
    pub size: u64,
    /// 这个文件现在不在了，内容是任务前快照里的样子。
    pub deleted: bool,
    /// 图片、PDF：`data:<类型>;base64,…`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
}

pub fn list_files(workspace: &Path) -> FileListing {
    let rules = GitignoreRules::from_workspace(workspace);
    let mut listing = FileListing::default();
    walk(workspace, workspace, &rules, &mut listing);
    listing.entries.sort_by(|a, b| a.path.cmp(&b.path));
    listing
}

fn walk(root: &Path, current: &Path, rules: &GitignoreRules, out: &mut FileListing) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let is_dir = file_type.is_dir();
        if rules.is_ignored(&rel_str, is_dir) {
            // `.git` 和 `.DS_Store` 本来就不算项目内容，不数进「藏了几个」。
            let name = entry.file_name();
            if name != ".git" && name != ".DS_Store" {
                out.hidden += 1;
            }
            continue;
        }
        if is_dir {
            walk(root, &path, rules, out);
        } else if file_type.is_file() {
            if out.entries.len() >= MAX_ENTRIES {
                out.truncated = true;
                return;
            }
            out.entries.push(FileEntry {
                path: rel_str,
                size: entry.metadata().map(|m| m.len()).unwrap_or(0),
            });
        }
    }
}

/// 只接受项目里的相对路径：不要绝对路径、`..`、`.git`。
pub fn check_relative(rel: &str) -> Result<(), String> {
    let path = Path::new(rel);
    if rel.trim().is_empty() || path.is_absolute() {
        return Err("只能看项目里的文件。".into());
    }
    let mut first = true;
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                if first && name == ".git" {
                    return Err("只能看项目里的文件。".into());
                }
            }
            _ => return Err("只能看项目里的文件。".into()),
        }
        first = false;
    }
    Ok(())
}

fn extension(rel: &str) -> String {
    Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn is_image(rel: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension(rel).as_str())
}

/// 交给 Quick Look 的那几种。
pub fn is_office(rel: &str) -> bool {
    OFFICE_EXTENSIONS.contains(&extension(rel).as_str())
}

fn mime_of(rel: &str) -> &'static str {
    match extension(rel).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "icns" => "image/icns",
        "tiff" => "image/tiff",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// base64（标准字母表，带 `=` 补齐）。
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn data_url(rel: &str, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", mime_of(rel), base64(bytes))
}

/// 项目里的文件 → 规范化过的绝对路径（不在项目里、指到外面的符号链接都拒掉）。
pub fn inside(workspace: &Path, rel: &str) -> Result<std::path::PathBuf, String> {
    check_relative(rel)?;
    let root = workspace.canonicalize().map_err(|e| format!("项目文件夹打不开：{e}"))?;
    let real = workspace.join(rel).canonicalize().map_err(|_| "这个文件不在了。".to_string())?;
    if !real.starts_with(&root) {
        return Err("只能看项目里的文件。".into());
    }
    Ok(real)
}

/// Quick Look 出一张缩略图（PNG 的 data URL）。只在 Mac 上有。
pub fn quicklook_thumbnail(file: &Path, size: u32) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("Quick Look 只在 Mac 上有。".into());
    }
    let out_dir = std::env::temp_dir().join(format!("peck-ql-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    // 最多等 20 秒：qlmanage 偶尔卡住，不能一直占着线程。
    let result = std::process::Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", &size.to_string(), "-o"])
        .arg(&out_dir)
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            let started = std::time::Instant::now();
            loop {
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                if started.elapsed() > std::time::Duration::from_secs(20) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::other("Quick Look 太久了"));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    let png = fs::read_dir(&out_dir).ok().and_then(|mut d| d.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png"))));
    let bytes = png.and_then(|p| fs::read(p).ok());
    let _ = fs::remove_dir_all(&out_dir);
    match (result, bytes) {
        (Ok(_), Some(b)) if !b.is_empty() => Ok(format!("data:image/png;base64,{}", base64(&b))),
        _ => Err("Quick Look 出不了这个文件的预览。".into()),
    }
}

/// 叫出系统的 Quick Look 窗口看这个文件（不等它关）。
pub fn quicklook_open(file: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("Quick Look 只在 Mac 上有。".into());
    }
    std::process::Command::new("/usr/bin/qlmanage")
        .arg("-p")
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // 窗口关了它就退出：另起一个线程等它，不留僵尸进程。
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        })
        .map_err(|e| format!("Quick Look 起不来：{e}"))
}

pub fn read_preview(workspace: &Path, rel: &str) -> Result<FilePreview, String> {
    check_relative(rel)?;
    let preview = |kind: &str, content: Option<String>, size: u64| FilePreview {
        path: rel.to_string(),
        kind: kind.to_string(),
        content,
        size,
        deleted: false,
        data_url: None,
    };
    let full = workspace.join(rel);
    let Ok(meta) = fs::metadata(&full) else {
        return Ok(preview("missing", None, 0));
    };
    if !meta.is_file() {
        return Ok(preview("missing", None, 0));
    }
    // 符号链接指到项目外的不读。
    let root = workspace
        .canonicalize()
        .map_err(|e| format!("项目文件夹打不开：{e}"))?;
    let real = full.canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&root) {
        return Err("只能看项目里的文件。".into());
    }
    let size = meta.len();
    let media = is_image(rel) || extension(rel) == "pdf";
    if media {
        if size > MAX_MEDIA_BYTES {
            return Ok(preview("large", None, size));
        }
        let bytes = fs::read(&real).map_err(|e| format!("读不了这个文件：{e}"))?;
        let kind = if is_image(rel) { "image" } else { "pdf" };
        return Ok(FilePreview { data_url: Some(data_url(rel, &bytes)), ..preview(kind, None, size) });
    }
    if is_office(rel) {
        return Ok(preview("office", None, size));
    }
    if size > MAX_PREVIEW_BYTES {
        return Ok(preview("large", None, size));
    }
    let bytes = fs::read(&real).map_err(|e| format!("读不了这个文件：{e}"))?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Ok(preview("binary", None, size));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(preview("text", Some(text), size)),
        Err(_) => Ok(preview("binary", None, size)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_hides_ignored_entries_and_preview_stays_inside_the_project() {
        let ws = std::env::temp_dir().join(format!("peck-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(ws.join("src")).unwrap();
        fs::create_dir_all(ws.join("node_modules/react")).unwrap();
        fs::create_dir_all(ws.join(".git")).unwrap();
        fs::create_dir_all(ws.join(".peck")).unwrap();
        fs::write(ws.join(".gitignore"), "node_modules/\n*.log\n.peck/\n").unwrap();
        fs::write(ws.join("src/main.ts"), "export const a = 1;\n").unwrap();
        fs::write(ws.join("node_modules/react/index.js"), "x").unwrap();
        fs::write(ws.join("debug.log"), "x").unwrap();
        fs::write(ws.join(".git/HEAD"), "ref").unwrap();
        fs::write(ws.join(".peck/tasks.md"), "- [ ] a\n").unwrap();
        fs::write(ws.join("logo.png"), [0u8, 1, 2]).unwrap();
        fs::write(ws.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();

        let listing = list_files(&ws);
        let paths: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".gitignore",
                ".peck/tasks.md",
                "blob.bin",
                "logo.png",
                "src/main.ts"
            ]
        );
        // node_modules（一个文件夹算一个）和 debug.log；.git 不数。
        assert_eq!(listing.hidden, 2);
        assert!(!listing.truncated);

        let text = read_preview(&ws, "src/main.ts").unwrap();
        assert_eq!(text.kind, "text");
        assert_eq!(text.content.as_deref(), Some("export const a = 1;\n"));
        let logo = read_preview(&ws, "logo.png").unwrap();
        assert_eq!(logo.kind, "image");
        assert_eq!(logo.data_url.as_deref(), Some("data:image/png;base64,AAEC"));
        fs::write(ws.join("doc.pdf"), b"%PDF-1.4").unwrap();
        let pdf = read_preview(&ws, "doc.pdf").unwrap();
        assert!(pdf.kind == "pdf" && pdf.data_url.unwrap().starts_with("data:application/pdf;base64,JVBERi0"));
        fs::write(ws.join("plan.docx"), [0u8, 1]).unwrap();
        assert_eq!(read_preview(&ws, "plan.docx").unwrap().kind, "office");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
        assert_eq!(read_preview(&ws, "blob.bin").unwrap().kind, "binary");
        assert_eq!(read_preview(&ws, "src/nope.ts").unwrap().kind, "missing");
        assert!(read_preview(&ws, "../outside.txt").is_err());
        assert!(read_preview(&ws, "/etc/hosts").is_err());
        assert!(read_preview(&ws, ".git/HEAD").is_err());

        let big = "x".repeat(MAX_PREVIEW_BYTES as usize + 1);
        fs::write(ws.join("big.txt"), big).unwrap();
        assert_eq!(read_preview(&ws, "big.txt").unwrap().kind, "large");

        #[cfg(unix)]
        {
            let outside =
                std::env::temp_dir().join(format!("peck-outside-{}", uuid::Uuid::new_v4()));
            fs::write(&outside, "secret").unwrap();
            std::os::unix::fs::symlink(&outside, ws.join("link.txt")).unwrap();
            assert!(read_preview(&ws, "link.txt").is_err());
            let _ = fs::remove_file(outside);
        }
        let _ = fs::remove_dir_all(ws);
    }
}
