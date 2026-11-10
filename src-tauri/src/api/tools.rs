use super::profiles::Protocol;
use crate::events::StreamType;
use crate::process::terminate_process_group;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;

pub const OUTPUT_LIMIT: usize = 24_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRecord {
    pub call_id: String,
    pub tool: ToolCall,
    pub status: String,
    pub output: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

pub fn definitions(protocol: &Protocol) -> Vec<Value> {
    [
        ("list_directory", "List a directory in the workspace. Paths are relative; use '.' for the root. Returns at most 300 entries; skips generated directories and .git.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})),
        ("read_file", "Read UTF-8 text inside the workspace, with line numbers. Use startLine/lineCount to page through large files; output is bounded. No access outside the workspace or to .git.", json!({"type":"object","properties":{"path":{"type":"string"},"startLine":{"type":"integer","minimum":1},"lineCount":{"type":"integer","minimum":1,"maximum":400}},"required":["path"]})),
        ("search", "Search literal text in workspace source files, respecting .gitignore. Returns matching paths and line numbers, up to 100 matches. An empty query lists source paths; optional path narrows the search.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"}},"required":["query"]})),
        ("apply_patch", "Apply exact text edits to UTF-8 files. Supply edits with path, oldText, newText. For a NEW file only, use oldText=''; for an existing file oldText must occur exactly once (read it first). Set delete=true with oldText equal to the entire file to delete. Repeated paths are applied in order to the staged text. The entire batch is validated before writing. Parent directories are created. Returns changed paths. This is JSON text replacement, not a unified diff.", json!({"type":"object","properties":{"edits":{"type":"array","minItems":1,"maxItems":30,"items":{"type":"object","properties":{"path":{"type":"string"},"oldText":{"type":"string"},"newText":{"type":"string"},"delete":{"type":"boolean"}},"required":["path","oldText","newText"]}}},"required":["edits"]})),
        ("run_command", "Run a non-interactive shell command in the workspace (optional relative cwd), with pipefail enabled. Each invocation starts at the workspace root unless cwd is set; cd does not persist. Git metadata is blocked, so do not use git status/diff; Peck captures task diffs independently. macOS sandbox: reads restricted to workspace/system/toolchains, writes to workspace and dedicated tool cache/temp only; no Keychain or inherited credentials. Cargo HOME/cache is isolated from the user's cache; fetch missing public dependencies with network=true before offline tests. No network by default; network=true permits dependency downloads. timeoutSeconds defaults to 60, max 180. Child processes are terminated on timeout/cancel, including background servers; do not use this tool for a persistent dev server. Output is bounded, with status and exitCode. Use file tools to edit.", json!({"type":"object","properties":{"command":{"type":"string"},"cwd":{"type":"string"},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":180},"network":{"type":"boolean"}},"required":["command"]})),
    ].into_iter().map(|(name, description, schema)| match protocol {
        Protocol::Anthropic => json!({"name":name,"description":description,"input_schema":schema}),
        Protocol::Responses => json!({"type":"function","name":name,"description":description,"parameters":schema,"strict":false}),
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => json!({"type":"function","function":{"name":name,"description":description,"parameters":schema}}),
    }).collect()
}

pub fn calls(message: &Value, protocol: &Protocol) -> Result<Vec<ToolCall>, String> {
    let mut calls = vec![];
    match protocol {
        Protocol::Anthropic => {
            for block in message["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_use" {
                    calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or("").into(),
                        name: block["name"].as_str().unwrap_or("").into(),
                        arguments: block["input"].clone(),
                    });
                }
            }
        }
        Protocol::Responses => {
            for item in message["output"].as_array().into_iter().flatten() {
                if item["type"] == "function_call" {
                    calls.push(ToolCall {
                        id: item["call_id"].as_str().unwrap_or("").into(),
                        name: item["name"].as_str().unwrap_or("").into(),
                        arguments: serde_json::from_str(item["arguments"].as_str().unwrap_or(""))
                            .map_err(|_| "工具参数不是有效 JSON。")?,
                    });
                }
            }
        }
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
            for tool in message["tool_calls"].as_array().into_iter().flatten() {
                calls.push(ToolCall {
                    id: tool["id"].as_str().unwrap_or("").into(),
                    name: tool["function"]["name"].as_str().unwrap_or("").into(),
                    arguments: serde_json::from_str(
                        tool["function"]["arguments"].as_str().unwrap_or(""),
                    )
                    .map_err(|_| "工具参数不是有效 JSON。")?,
                });
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    if calls.len() > 16
        || calls.iter().any(|t| {
            t.id.is_empty() || t.name.is_empty() || !t.arguments.is_object() || !ids.insert(&t.id)
        })
    {
        return Err("工具调用缺少 ID/名称、ID 重复或超过单轮 16 项；未执行本轮工具。".into());
    }
    Ok(calls)
}

pub fn append_results(messages: &mut Vec<Value>, protocol: &Protocol, results: &[ToolRecord]) {
    match protocol {
        Protocol::Anthropic => messages.push(json!({"role":"user","content":results.iter().map(|r| json!({"type":"tool_result","tool_use_id":r.tool.id,"content":r.output,"is_error":r.status!="completed"})).collect::<Vec<_>>()})),
        Protocol::Responses => messages.extend(results.iter().map(|r| json!({"type":"function_call_output","call_id":r.tool.id,"output":r.output}))),
        Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => messages.extend(results.iter().map(|r| json!({"role":"tool","tool_call_id":r.tool.id,"content":r.output}))),
    }
}

// Resolve missing files via their nearest existing ancestor; symlinks may not escape the root.
pub fn workspace_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir) || c.as_os_str() == ".git")
    {
        return Err("路径必须在工作区内，不能包含 .. 或 .git。".into());
    }
    let joined = root.join(path);
    let mut ancestor = joined.as_path();
    while !ancestor.exists() {
        // Broken symlinks must not become file creation targets either.
        if std::fs::symlink_metadata(ancestor).is_ok() {
            return Err("路径包含不可访问的符号链接。".into());
        }
        ancestor = ancestor.parent().ok_or("路径无效。")?;
    }
    let canonical = ancestor.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(root) {
        return Err("符号链接指向工作区外，无法访问。".into());
    }
    let suffix = joined.strip_prefix(ancestor).map_err(|e| e.to_string())?;
    Ok(if suffix.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(suffix)
    })
}

pub fn clipped(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[输出已截断，请缩小范围或分页读取]");
    }
    text
}

pub async fn execute(
    root: &Path,
    tool: &ToolCall,
    scope: &[String],
    cancel: &mut watch::Receiver<bool>,
    emit: impl FnMut(StreamType, String),
) -> Result<String, String> {
    if *cancel.borrow() {
        return Err("已取消，未执行工具。".into());
    }
    let args = &tool.arguments;
    let path = args["path"].as_str().unwrap_or(".");
    match tool.name.as_str() {
        "list_directory" => {
            let dir = workspace_path(root, path)?;
            let mut entries = std::fs::read_dir(dir)
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            entries.sort_by_key(|e| e.file_name());
            Ok(entries
                .into_iter()
                .filter(|e| {
                    ![".git", "node_modules", "target", ".DS_Store"]
                        .contains(&e.file_name().to_string_lossy().as_ref())
                })
                .take(300)
                .map(|e| {
                    format!(
                        "{}{}",
                        e.file_name().to_string_lossy(),
                        if e.path().is_dir() { "/" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "read_file" => {
            let file = workspace_path(root, path)?;
            if std::fs::metadata(&file).map_err(|e| e.to_string())?.len() > 2_000_000 {
                return Err("文件超过 2MB，请使用命令做有界读取。".into());
            }
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let start = args["startLine"].as_u64().unwrap_or(1).max(1) as usize;
            let count = args["lineCount"].as_u64().unwrap_or(200).clamp(1, 400) as usize;
            Ok(clipped(
                text.lines()
                    .enumerate()
                    .skip(start - 1)
                    .take(count)
                    .map(|(i, s)| format!("{}: {s}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n"),
                OUTPUT_LIMIT,
            ))
        }
        "search" => {
            let base = workspace_path(root, path)?;
            let query = args["query"].as_str().ok_or("缺少 query。")?;
            let rules = crate::watch::GitignoreRules::from_workspace(root);
            let files = crate::watch::scan_workspace_files(root, &rules);
            let mut found = vec![];
            for (rel, stamp) in files {
                if !root.join(&rel).starts_with(&base)
                    || stamp.len > 1_000_000
                    || rel
                        .split('/')
                        .any(|p| ["node_modules", "target", "dist"].contains(&p))
                    || rel.starts_with(".peck/runtime/")
                {
                    continue;
                }
                if query.is_empty() {
                    found.push(rel);
                } else if let Ok(text) = std::fs::read_to_string(root.join(&rel)) {
                    for (i, line) in text.lines().enumerate() {
                        if line.contains(query) {
                            found.push(format!("{rel}:{}: {}", i + 1, clipped(line.into(), 1000)));
                        }
                        if found.len() >= 100 {
                            break;
                        }
                    }
                }
                if found.len() >= 100 {
                    break;
                }
            }
            Ok(clipped(found.join("\n"), OUTPUT_LIMIT))
        }
        "apply_patch" => {
            let edits = args["edits"].as_array().ok_or("缺少 edits 数组。")?;
            if edits.is_empty() || edits.len() > 30 {
                return Err("每次补丁需包含 1～30 处编辑。".into());
            }
            let mut staged: std::collections::BTreeMap<PathBuf, (String, Option<String>)> =
                std::collections::BTreeMap::new();
            for edit in edits {
                let rel = edit["path"].as_str().ok_or("补丁缺少 path。")?;
                let file = workspace_path(root, rel)?;
                if !scope.is_empty() && !scope.iter().any(|p| file.starts_with(root.join(p))) {
                    return Err(format!("{rel}: 超出本次委派的写入范围。"));
                }
                let old = edit["oldText"].as_str().ok_or("补丁缺少 oldText。")?;
                let new = edit["newText"].as_str().ok_or("补丁缺少 newText。")?;
                let delete = edit["delete"].as_bool().unwrap_or(false);
                let current = match staged.get(&file) {
                    Some((_, content)) => content.clone(),
                    None if file.exists() => {
                        Some(std::fs::read_to_string(&file).map_err(|e| e.to_string())?)
                    }
                    None => None,
                };
                let content = if let Some(text) = current {
                    if old.is_empty() || text.matches(old).count() != 1 || (delete && old != text) {
                        return Err(format!(
                            "{rel}: oldText 必须精确且唯一匹配；删除需匹配全文。未应用本批补丁。"
                        ));
                    }
                    text.replacen(old, new, 1)
                } else {
                    if !old.is_empty() || delete {
                        return Err(format!("{rel}: 新文件需 oldText 为空且不能删除。"));
                    }
                    new.into()
                };
                staged.insert(
                    file,
                    (rel.to_string(), if delete { None } else { Some(content) }),
                );
            }
            let mut changed = vec![];
            for (file, (rel, content)) in staged {
                let result = if let Some(content) = content {
                    std::fs::create_dir_all(file.parent().ok_or("路径无效。")?)
                        .and_then(|_| std::fs::write(&file, content))
                } else if file.exists() {
                    std::fs::remove_file(&file)
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    return Err(format!(
                        "写入 {rel} 失败：{e}；此前已写入：{}",
                        changed.join(", ")
                    ));
                }
                changed.push(rel);
            }
            Ok(format!("已应用补丁：{}", changed.join(", ")))
        }
        "run_command" => run_command(root, args, scope, cancel, emit).await,
        _ => Err(format!("未知工具 {}；Worker 不支持委派。", tool.name)),
    }
}

async fn run_command(
    root: &Path,
    args: &Value,
    scope: &[String],
    cancel: &mut watch::Receiver<bool>,
    mut emit: impl FnMut(StreamType, String),
) -> Result<String, String> {
    let command = args["command"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("缺少 command。")?;
    let cwd = workspace_path(root, args["cwd"].as_str().unwrap_or("."))?;
    let runtime = crate::mode::app_data_dir()
        .join("worker-runtime")
        .join(crate::workspace::workspace_hash(root));
    std::fs::create_dir_all(runtime.join("home")).map_err(|e| e.to_string())?;
    let runtime = runtime.canonicalize().map_err(|e| e.to_string())?;
    let tool_home = runtime.join("home");
    let temp = runtime.join(format!("tmp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let temp = temp.canonicalize().map_err(|e| e.to_string())?;
    let home = std::env::var("HOME").unwrap_or_default();
    let mut extra_paths = vec![];
    // Finder-launched apps do not inherit the user's nvm shell setup.
    if let Ok(entries) = std::fs::read_dir(PathBuf::from(&home).join(".nvm/versions/node")) {
        let mut versions = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("bin/node").is_file())
            .collect::<Vec<_>>();
        versions.sort_by_key(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .trim_start_matches('v')
                .split('.')
                .map(|v| v.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
        extra_paths.extend(
            versions
                .into_iter()
                .rev()
                .map(|p| p.join("bin").to_string_lossy().into_owned()),
        );
    }
    let mut reads = vec![
        root.to_path_buf(),
        temp.clone(),
        tool_home.clone(),
        PathBuf::from("/System"),
        PathBuf::from("/usr"),
        PathBuf::from("/bin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/Library/Developer"),
        PathBuf::from("/Applications/Xcode.app"),
        PathBuf::from("/opt/homebrew"),
        PathBuf::from("/private/etc"),
        PathBuf::from("/dev"),
    ];
    for p in [
        ".cargo/bin",
        ".rustup",
        ".nvm/versions",
        "Library/pnpm",
        "Library/Caches/node/corepack",
    ] {
        let p = PathBuf::from(&home).join(p);
        if p.exists() {
            reads.push(p.canonicalize().map_err(|e| e.to_string())?);
        }
    }
    let literals = reads
        .iter()
        .map(|p| format!("(subpath {})", json!(p.to_string_lossy())))
        .collect::<Vec<_>>()
        .join(" ");
    let writes = if scope.is_empty() {
        format!("(subpath {})", json!(root.to_string_lossy()))
    } else {
        scope
            .iter()
            .map(|p| {
                workspace_path(root, p).map(|p| format!("(subpath {})", json!(p.to_string_lossy())))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(" ")
    };
    let profile = format!("(version 1)(deny default)(import \"dyld-support.sb\")(allow process*)(allow signal)(allow sysctl-read)(allow mach-lookup)(deny mach-lookup (global-name \"com.apple.securityd\") (global-name \"com.apple.securityd.xpc\"))(allow file-read-metadata)(allow file-read* file-map-executable {literals})(allow file-write* {writes} (subpath {}) (subpath {}) (literal \"/dev/null\"))(deny file-read* file-write* (subpath {})){}",json!(temp.to_string_lossy()),json!(tool_home.to_string_lossy()),json!(root.join(".git").to_string_lossy()),if args["network"]==true {"(allow network*)"} else {""});
    let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
    cmd.args(["-p", &profile, "/bin/sh", "-o", "pipefail", "-c", command])
        .current_dir(cwd)
        .env_clear()
        .env("PATH", crate::agent::build_augmented_path(&extra_paths))
        .env("HOME", &tool_home)
        .env("TMPDIR", &temp)
        .env("CARGO_HOME", tool_home.join(".cargo"))
        .env("RUSTUP_HOME", PathBuf::from(&home).join(".rustup"))
        .env("LANG", "en_US.UTF-8")
        .env("CI", "1")
        .env("COREPACK_ENABLE_AUTO_PIN", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    if *cancel.borrow() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err("已取消，未启动命令。".into());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动隔离命令，未退回无隔离执行：{e}"))?;
    let pgid = child.id();
    let stdout = child.stdout.take().ok_or("无法读取 stdout。")?;
    let stderr = child.stderr.take().ok_or("无法读取 stderr。")?;
    let out = tokio::spawn(async move {
        let mut b = vec![];
        let _ = stdout
            .take((OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut b)
            .await;
        b
    });
    let err = tokio::spawn(async move {
        let mut b = vec![];
        let _ = stderr
            .take((OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut b)
            .await;
        b
    });
    let timeout = args["timeoutSeconds"].as_u64().unwrap_or(60).clamp(1, 180);
    let (status, code) = tokio::select! {
        biased;
        _=cancel.changed()=> ("cancelled",None),
        _=tokio::time::sleep(std::time::Duration::from_secs(timeout))=> ("timedOut",None),
        r=child.wait()=> { let s=r.map_err(|e|e.to_string())?; (if s.success(){"completed"}else{"failed"},s.code()) },
    };
    terminate_process_group(pgid).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    let stdout = clipped(
        String::from_utf8_lossy(&out.await.unwrap_or_default()).into(),
        OUTPUT_LIMIT,
    );
    let stderr = clipped(
        String::from_utf8_lossy(&err.await.unwrap_or_default()).into(),
        OUTPUT_LIMIT,
    );
    emit(StreamType::Stdout, stdout.clone());
    if !stderr.is_empty() {
        emit(StreamType::Stderr, stderr.clone());
    }
    let _ = std::fs::remove_dir_all(temp);
    Ok(json!({"status":status,"exitCode":code,"stdout":stdout,"stderr":stderr}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn patches_validate_batch_and_paths_cannot_escape_workspace() {
        let dir = std::env::temp_dir().join(format!("peck-tools-{}", uuid::Uuid::new_v4()));
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("a.txt"), "hello\n").unwrap();
        std::fs::write(dir.join("outside.txt"), "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dir, root.join("escape")).unwrap();
        assert!(workspace_path(&root, "../outside.txt").is_err());
        assert!(workspace_path(&root, "/etc/passwd").is_err());
        assert!(workspace_path(&root, ".git/index").is_err());
        assert!(workspace_path(&root, "escape/outside.txt").is_err());
        let (_tx, mut rx) = watch::channel(false);
        let mut tool = ToolCall {
            id: "p1".into(),
            name: "apply_patch".into(),
            arguments: json!({"edits":[{"path":"a.txt","oldText":"hello","newText":"changed"},{"path":"missing.txt","oldText":"oops","newText":"no"}]}),
        };
        assert!(execute(&root, &tool, &[], &mut rx, |_, _| {})
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "hello\n"
        );
        tool.arguments = json!({"edits":[{"path":"a.txt","oldText":"hello","newText":"changed"},{"path":"nested/new.txt","oldText":"","newText":"中文\n"}]});
        execute(&root, &tool, &[], &mut rx, |_, _| {})
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "changed\n"
        );
        tool.arguments = json!({"edits":[{"path":"a.txt","oldText":"changed","newText":"staged"},{"path":"a.txt","oldText":"absent","newText":"no"}]});
        assert!(execute(&root, &tool, &[], &mut rx, |_, _| {})
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "changed\n"
        );
        tool.arguments = json!({"edits":[{"path":"a.txt","oldText":"changed","newText":"staged"},{"path":"a.txt","oldText":"staged","newText":"final"}]});
        let result = execute(&root, &tool, &[], &mut rx, |_, _| {})
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "final\n"
        );
        assert_eq!(result, "已应用补丁：a.txt");
        tool.arguments = json!({"edits":[{"path":"nested/new.txt","oldText":"中文\n","newText":"","delete":true}]});
        execute(&root, &tool, &[], &mut rx, |_, _| {})
            .await
            .unwrap();
        assert!(!root.join("nested/new.txt").exists());
        assert_eq!(
            clipped("中文abcdef".into(), 4),
            "中\n[输出已截断，请缩小范围或分页读取]"
        );
    }

    #[test]
    fn all_protocols_keep_ids_order_and_error_results() {
        for protocol in [
            Protocol::Anthropic,
            Protocol::ChatCompletions,
            Protocol::Responses,
            Protocol::Gemini,
            Protocol::Qwen,
        ] {
            let message = match protocol {
                Protocol::Anthropic => {
                    json!({"role":"assistant","content":[{"type":"thinking","thinking":"x","signature":"keep"},{"type":"tool_use","id":"t1","name":"read_file","input":{"path":"x"}}]})
                }
                Protocol::Responses => {
                    json!({"output":[{"type":"reasoning","encrypted_content":"keep"},{"type":"function_call","call_id":"t1","name":"read_file","arguments":"{\"path\":\"x\"}"}]})
                }
                Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
                    json!({"role":"assistant","content":"","reasoning_content":"keep","tool_calls":[{"id":"t1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"x\"}"}}]})
                }
            };
            let parsed = calls(&message, &protocol).unwrap();
            let mut messages = vec![message.clone()];
            append_results(
                &mut messages,
                &protocol,
                &[ToolRecord {
                    call_id: "c".into(),
                    tool: parsed[0].clone(),
                    status: "failed".into(),
                    output: "missing".into(),
                    started_at: 1,
                    finished_at: Some(2),
                }],
            );
            assert_eq!(messages[0], message);
            match protocol {
                Protocol::Anthropic => {
                    assert_eq!(messages[1]["content"][0]["tool_use_id"], "t1");
                    assert_eq!(messages[1]["content"][0]["is_error"], true);
                }
                Protocol::Responses => {
                    assert_eq!(messages[1]["call_id"], "t1");
                    assert_eq!(messages[1]["type"], "function_call_output");
                }
                Protocol::ChatCompletions | Protocol::Gemini | Protocol::Qwen => {
                    assert_eq!(messages[1]["tool_call_id"], "t1")
                }
            }
        }
        assert!(calls(
            &json!({"tool_calls":[{"id":"t","function":{"name":"x","arguments":"{"}}]}),
            &Protocol::ChatCompletions
        )
        .is_err());
    }

    #[tokio::test]
    async fn commands_are_confined_and_do_not_inherit_secrets() {
        let dir = std::env::temp_dir().join(format!("peck-command-test-{}", uuid::Uuid::new_v4()));
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let dir = dir.canonicalize().unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(dir.join("secret.txt"), "OUTSIDE_SENTINEL").unwrap();
        std::fs::write(root.join("inside.txt"), "INSIDE_SENTINEL").unwrap();
        let (_tx, mut rx) = watch::channel(false);
        let args = json!({"command":format!("cat inside.txt; cat {}; echo overwrite > {}; env; node --version && pnpm --version && cargo --version",dir.join("secret.txt").display(),dir.join("secret.txt").display())});
        let output = run_command(&root, &args, &[], &mut rx, |_, _| {})
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["exitCode"], 0, "{output}");
        assert!(
            value["stdout"]
                .as_str()
                .unwrap()
                .contains("INSIDE_SENTINEL"),
            "{output}"
        );
        assert!(!value["stdout"]
            .as_str()
            .unwrap()
            .contains("OUTSIDE_SENTINEL"));
        assert!(!value["stdout"].as_str().unwrap().contains("PECK_API_MODE"));
        let output = run_command(
            &root,
            &json!({"command":"false | cat"}),
            &[],
            &mut rx,
            |_, _| {},
        )
        .await
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["exitCode"], 1, "{output}");
        assert_eq!(value["status"], "failed", "{output}");
        assert_eq!(
            std::fs::read_to_string(dir.join("secret.txt")).unwrap(),
            "OUTSIDE_SENTINEL"
        );
    }

    #[tokio::test]
    async fn cancel_and_timeout_terminate_command_descendants() {
        let root =
            std::env::temp_dir().join(format!("peck-command-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (tx, mut rx) = watch::channel(false);
        let args =
            json!({"command":"(sleep 2; echo leaked > descendant.txt) & wait","timeoutSeconds":10});
        let path = root.clone();
        let running = tokio::spawn(async move {
            run_command(&path, &args, &[], &mut rx, |_, _| {})
                .await
                .unwrap()
        });
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        tx.send(true).unwrap();
        let output = tokio::time::timeout(std::time::Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap()["status"],
            "cancelled"
        );
        let (_tx, mut rx) = watch::channel(false);
        let timed = run_command(
            &root,
            &json!({"command":"sleep 5","timeoutSeconds":1}),
            &[],
            &mut rx,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&timed).unwrap()["status"],
            "timedOut"
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert!(!root.join("descendant.txt").exists());
    }
    #[tokio::test]
    async fn delegated_write_scope_covers_patches_commands_and_internal_symlinks() {
        let root = std::env::temp_dir().join(format!("peck-scope-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("allowed")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("other/keep.txt"), "keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("other"), root.join("allowed/link")).unwrap();
        let scope = vec!["allowed".into()];
        let (_tx, mut rx) = watch::channel(false);
        let mut tool = ToolCall {
            id: "scope".into(),
            name: "apply_patch".into(),
            arguments: json!({"edits":[{"path":"allowed/link/keep.txt","oldText":"keep","newText":"bad"}]}),
        };
        assert!(execute(&root, &tool, &scope, &mut rx, |_, _| {})
            .await
            .is_err());
        tool.arguments = json!({"edits":[{"path":"allowed/new.txt","oldText":"","newText":"ok"}]});
        execute(&root, &tool, &scope, &mut rx, |_, _| {})
            .await
            .unwrap();
        let out = run_command(
            &root,
            &json!({"command":"echo bad > other/keep.txt; echo ok > allowed/command.txt"}),
            &scope,
            &mut rx,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["exitCode"], 0);
        assert_eq!(
            std::fs::read_to_string(root.join("other/keep.txt")).unwrap(),
            "keep"
        );
        assert!(root.join("allowed/command.txt").is_file());
    }
}
