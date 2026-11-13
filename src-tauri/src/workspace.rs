use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub name: String,
    pub last_opened_at: i64,
    pub git_branch: Option<String>,
    pub gitignore_configured: bool,
    /// 「移出列表」了：文件夹、任务、快照都不动，重新打开这个文件夹就回来。
    #[serde(default)]
    pub hidden: bool,
    /// 项目自己的设置（右键 → 项目设置）。
    #[serde(default)]
    pub settings: ProjectSettings,
}

/// 项目设置：在这个项目里发任务默认谁来干、哪一档权限（空着就跟全局的）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSettings {
    pub who: Option<String>,
    pub permission: Option<String>,
    /// 站主不要 Peck 猜的验证命令（v0.6）：中栏不再问；项目设置里照样能写 `.peck/verify.toml`。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub verify_dismissed: bool,
    /// 站主确认过的 `.peck/verify.toml`（原文的 md5）。项目自带的、模型改过的，确认前不替模型跑、不起 server。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_approved: Option<String>,
}

/// Normalize a workspace path string (canonicalizing when possible, stripping `/private` prefix on macOS tmp if needed for display consistency).
pub fn normalize_workspace_path(path: &Path) -> PathBuf {
    if let Ok(canon) = fs::canonicalize(path) {
        canon
    } else {
        path.to_path_buf()
    }
}

/// Compute the 32-character lowercase hex MD5 digest of a workspace path.
/// Used for `<app_data_dir>/shadow/<workspace_md5>/` and workspace IDs.
pub fn workspace_hash(workspace_path: &Path) -> String {
    let normalized = normalize_workspace_path(workspace_path);
    md5_hex(normalized.to_string_lossy().as_bytes())
}

/// Check if a `.gitignore` content string already ignores `.peck/`.
pub fn gitignore_contains_peck(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        matches!(
            trimmed,
            ".peck" | ".peck/" | "/.peck" | "/.peck/" | ".peck/**" | "/.peck/**"
        )
    })
}

/// Ensure `.peck/` is present in `<workspace_path>/.gitignore` idempotently.
/// Returns `Ok(true)` if `.peck/` was newly added, or `Ok(false)` if it was already present.
pub fn ensure_peck_in_gitignore(workspace_path: &Path) -> Result<bool, String> {
    if !workspace_path.exists() {
        return Err(format!(
            "Workspace directory does not exist: {}",
            workspace_path.display()
        ));
    }

    let gitignore_path = workspace_path.join(".gitignore");
    let existing = if gitignore_path.exists() {
        fs::read_to_string(&gitignore_path)
            .map_err(|e| format!("Failed to read .gitignore: {e}"))?
    } else {
        String::new()
    };

    if gitignore_contains_peck(&existing) {
        return Ok(false);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() && !updated.ends_with("\n\n") {
        updated.push('\n');
    }
    updated.push_str("# Peck runtime handoff & task artifacts\n.peck/\n");

    fs::write(&gitignore_path, updated).map_err(|e| format!("Failed to write .gitignore: {e}"))?;

    Ok(true)
}

/// Detect the current Git branch of `workspace_path` without modifying the repository.
pub fn detect_git_branch(workspace_path: &Path) -> Option<String> {
    // 1. Fast path: inspect `.git/HEAD` in workspace_path or ancestor directories
    let mut cursor: Option<&Path> = Some(workspace_path);
    while let Some(dir) = cursor {
        let head_file = dir.join(".git").join("HEAD");
        if head_file.is_file() {
            if let Ok(raw) = fs::read_to_string(&head_file) {
                let trimmed = raw.trim();
                if let Some(ref_path) = trimmed.strip_prefix("ref: ") {
                    let branch = ref_path
                        .trim()
                        .strip_prefix("refs/heads/")
                        .unwrap_or(ref_path.trim());
                    if !branch.is_empty() {
                        return Some(branch.to_string());
                    }
                } else if trimmed.len() >= 7 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(trimmed[..7].to_string());
                }
            }
        }
        cursor = dir.parent();
    }

    // 2. Fallback: ask `git symbolic-ref --short HEAD` or `git rev-parse --abbrev-ref HEAD`
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(workspace_path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }

    None
}

/// Open a workspace directory:
/// 1. Validates directory existence;
/// 2. Detects Git status and current branch;
/// 3. Automatically injects `.peck/` into `.gitignore` idempotently;
/// 4. Returns a populated `Workspace` descriptor.
pub fn open_workspace(path_str: &str) -> Result<Workspace, String> {
    let raw_path = Path::new(path_str.trim());
    if !raw_path.exists() {
        return Err(format!("工作区目录不存在: {path_str}"));
    }
    if !raw_path.is_dir() {
        return Err(format!("路径不是有效目录: {path_str}"));
    }

    let hash = workspace_hash(raw_path);
    let id = format!("ws-{}", &hash[..12]);

    let name = raw_path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace")
        .to_string();

    let git_branch = detect_git_branch(raw_path);
    let gitignore_configured = ensure_peck_in_gitignore(raw_path).is_ok();

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(1762340000);

    Ok(Workspace {
        id,
        path: path_str.trim().to_string(),
        name,
        last_opened_at: now_ts,
        git_branch,
        gitignore_configured,
        hidden: false,
        settings: ProjectSettings::default(),
    })
}

/// Pure-Rust RFC 1321 MD5 implementation returning a 32-character lowercase hex string.
pub fn md5_hex(data: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }

        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut d = d0;

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = temp;
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());

    let mut hex = String::with_capacity(32);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_rfc1321_vectors_and_workspace_hash() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");

        let p = Path::new("/tmp/peck-example-ws");
        let h1 = workspace_hash(p);
        let h2 = workspace_hash(p);
        assert_eq!(h1.len(), 32);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_ensure_peck_in_gitignore_idempotent() {
        let temp_dir =
            std::env::temp_dir().join(format!("peck-ws-gitignore-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        // 1st call creates .gitignore with .peck/
        let added_first = ensure_peck_in_gitignore(&temp_dir).unwrap();
        assert!(added_first);

        let content_first = fs::read_to_string(temp_dir.join(".gitignore")).unwrap();
        assert!(gitignore_contains_peck(&content_first));

        // 2nd call is idempotent and does not duplicate .peck/
        let added_second = ensure_peck_in_gitignore(&temp_dir).unwrap();
        assert!(!added_second);

        let content_second = fs::read_to_string(temp_dir.join(".gitignore")).unwrap();
        assert_eq!(content_first, content_second);
        let peck_count = content_second
            .lines()
            .filter(|l| l.trim() == ".peck/")
            .count();
        assert_eq!(peck_count, 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_ensure_peck_in_gitignore_preserves_existing_lines_without_trailing_newline() {
        let temp_dir =
            std::env::temp_dir().join(format!("peck-ws-existing-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let gitignore_path = temp_dir.join(".gitignore");
        fs::write(&gitignore_path, "node_modules/\ndist/").unwrap();

        let added = ensure_peck_in_gitignore(&temp_dir).unwrap();
        assert!(added);

        let updated = fs::read_to_string(&gitignore_path).unwrap();
        assert!(updated.contains("node_modules/\n"));
        assert!(updated.contains("dist/\n"));
        assert!(updated.contains(".peck/\n"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_open_workspace_detects_git_branch_and_injects_gitignore() {
        let temp_dir = std::env::temp_dir().join(format!("peck-ws-open-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        // Simulate a Git repo with HEAD pointing to refs/heads/feat/shadow-git
        let git_dir = temp_dir.join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feat/shadow-git\n").unwrap();

        let ws = open_workspace(&temp_dir.to_string_lossy()).unwrap();
        assert_eq!(ws.git_branch, Some("feat/shadow-git".to_string()));
        assert!(ws.gitignore_configured);
        assert!(temp_dir.join(".gitignore").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
