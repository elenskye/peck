//! 右栏 FILES：只看不改。照忽略规则列项目里的文件，读一个文件给预览。

use serde::Serialize;
use std::fs;
use std::path::{Component, Path};

use crate::watch::GitignoreRules;

/// 列表最多给这么多个文件，再多就写「太多了」。
pub const MAX_ENTRIES: usize = 4000;
/// 超过这个大小不预览。
pub const MAX_PREVIEW_BYTES: u64 = 400 * 1024;

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

fn is_image(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn read_preview(workspace: &Path, rel: &str) -> Result<FilePreview, String> {
    check_relative(rel)?;
    let preview = |kind: &str, content: Option<String>, size: u64| FilePreview {
        path: rel.to_string(),
        kind: kind.to_string(),
        content,
        size,
        deleted: false,
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
    if is_image(rel) {
        return Ok(preview("image", None, size));
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
        assert_eq!(read_preview(&ws, "logo.png").unwrap().kind, "image");
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
