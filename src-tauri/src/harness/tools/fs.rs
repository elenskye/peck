//! 读、列、找、整份写：`list_directory`、`read_file`、`find_files`、`write_file`。

use super::{file_diff, resolve, str_arg, u64_arg, ToolContext, ToolResult};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;

pub const LIST_LIMIT: usize = 300;
pub const READ_LINES: usize = 2000;
pub const READ_BYTES: usize = 50 * 1024;
pub const LINE_CHARS: usize = 2000;
pub const FIND_LIMIT: usize = 100;
/// 超过这么大的文件不整个读进内存：边读边数行，只留要的那一段（也不记「读过」，不能用 write_file 整个覆盖）。
pub const WHOLE_FILE: u64 = 8 * 1024 * 1024;

/// 遍历项目：遵守 .gitignore（没有 git 也照读）、带隐藏文件，但不进 `.git`。
pub fn walker(base: &Path, depth: Option<usize>) -> ignore::WalkBuilder {
    let mut w = ignore::WalkBuilder::new(base);
    w.hidden(false)
        .require_git(false)
        .git_global(false)
        .max_depth(depth)
        .filter_entry(|e| e.file_name() != ".git")
        .sort_by_file_name(|a, b| a.cmp(b));
    w
}

pub fn list_directory(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let dir = resolve(&ctx.root, str_arg(input, "path").unwrap_or("."))?;
    if !dir.is_dir() {
        return Err(format!("{} 不是文件夹。", ctx.rel(&dir)));
    }
    let depth = u64_arg(input, "depth").unwrap_or(1).clamp(1, 3) as usize;
    let mut lines = Vec::new();
    let mut more = 0usize;
    for entry in walker(&dir, Some(depth)).build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        if lines.len() >= LIST_LIMIT {
            more += 1;
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let name = entry.file_name().to_string_lossy();
        lines.push(format!(
            "{}{}{}",
            "  ".repeat(entry.depth() - 1),
            name,
            if is_dir { "/" } else { "" }
        ));
    }
    let mut text = format!("{}/\n", ctx.rel(&dir));
    if lines.is_empty() {
        text.push_str("（空的）");
    } else {
        text.push_str(&lines.join("\n"));
    }
    if more > 0 {
        let _ = write!(text, "\n（只列了前 {LIST_LIMIT} 条，还有 {more} 条没列；缩小 path 或减小 depth）");
    }
    Ok(ToolResult::ok(text))
}

pub fn read_file(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    read_file_within(ctx, input, WHOLE_FILE)
}

fn read_file_within(ctx: &ToolContext, input: &Value, whole_file: u64) -> Result<ToolResult, String> {
    let raw = str_arg(input, "path").ok_or("缺 path。")?;
    let given = Path::new(raw.trim());
    // 落盘的命令输出在项目外，也能读。
    let file = if given.is_absolute() && ctx.is_spill(given) && !raw.contains("..") {
        given.to_path_buf()
    } else {
        resolve(&ctx.root, raw)?
    };
    if file.is_dir() {
        return Err(format!("{} 是文件夹，用 list_directory 看。", ctx.rel(&file)));
    }
    let size = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size > whole_file {
        return read_large(ctx, &file, size, input);
    }
    let bytes = std::fs::read(&file).map_err(|_| format!("读不到 {}（不存在或没权限）。", ctx.rel(&file)))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Ok(ToolResult::ok(format!("{} 是二进制文件（{} 字节），不显示内容。", ctx.rel(&file), bytes.len())));
    }
    ctx.remember_read(&file, &bytes);
    let text = String::from_utf8_lossy(&bytes);
    let total = text.lines().count();
    let start = u64_arg(input, "start_line").unwrap_or(1).max(1) as usize;
    let count = u64_arg(input, "line_count").unwrap_or(READ_LINES as u64).clamp(1, READ_LINES as u64) as usize;
    if total == 0 {
        return Ok(ToolResult::ok(format!("{} 是空文件。", ctx.rel(&file))));
    }
    if start > total {
        return Err(format!("{} 只有 {total} 行，第 {start} 行不存在。", ctx.rel(&file)));
    }
    let mut out = String::new();
    let mut last = start - 1;
    let mut cut_by_size = false;
    for (i, line) in text.lines().enumerate().skip(start - 1).take(count) {
        let shown: String = if line.chars().count() > LINE_CHARS {
            format!("{}…（这一行太长，截断了）", line.chars().take(LINE_CHARS).collect::<String>())
        } else {
            line.to_string()
        };
        let row = format!("{:>6}\t{}\n", i + 1, shown);
        if out.len() + row.len() > READ_BYTES && last >= start {
            cut_by_size = true;
            break;
        }
        out.push_str(&row);
        last = i + 1;
    }
    if last < total {
        let _ = write!(
            out,
            "（共 {total} 行，{}只显示到第 {last} 行；下次从第 {} 行读）",
            if cut_by_size { "超过 50KB，" } else { "" },
            last + 1
        );
    }
    Ok(ToolResult::ok(out))
}

/// 大文件：一块块读、边读边数行，内存里只留要的那几行（一行最多留 LINE_CHARS 个字）。
fn read_large(ctx: &ToolContext, file: &Path, size: u64, input: &Value) -> Result<ToolResult, String> {
    use std::io::{BufRead, BufReader};
    let cant = || format!("读不到 {}（不存在或没权限）。", ctx.rel(file));
    let mut reader = BufReader::with_capacity(64 * 1024, std::fs::File::open(file).map_err(|_| cant())?);
    if reader.fill_buf().map_err(|_| cant())?.iter().take(8192).any(|b| *b == 0) {
        return Ok(ToolResult::ok(format!("{} 是二进制文件（{size} 字节），不显示内容。", ctx.rel(file))));
    }
    let start = u64_arg(input, "start_line").unwrap_or(1).max(1) as usize;
    let count = u64_arg(input, "line_count").unwrap_or(READ_LINES as u64).clamp(1, READ_LINES as u64) as usize;
    let keep = LINE_CHARS * 4;
    let (mut out, mut last, mut cut_by_size, mut stop) = (String::new(), start - 1, false, false);
    let (mut idx, mut cur, mut long, mut last_byte) = (1usize, Vec::<u8>::new(), false, b'\n');
    let finish = |idx: usize, cur: &[u8], long: bool, out: &mut String, last: &mut usize, cut: &mut bool, stop: &mut bool| {
        if *stop || idx < start || idx >= start + count {
            return;
        }
        let text = String::from_utf8_lossy(cur);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        let shown = if long || text.chars().count() > LINE_CHARS {
            format!("{}…（这一行太长，截断了）", text.chars().take(LINE_CHARS).collect::<String>())
        } else {
            text.to_string()
        };
        let row = format!("{idx:>6}\t{shown}\n");
        if out.len() + row.len() > READ_BYTES && *last >= start {
            *cut = true;
            *stop = true;
            return;
        }
        out.push_str(&row);
        *last = idx;
    };
    loop {
        let buf = reader.fill_buf().map_err(|_| cant())?;
        if buf.is_empty() {
            break;
        }
        let len = buf.len();
        for &b in buf {
            if b == b'\n' {
                finish(idx, &cur, long, &mut out, &mut last, &mut cut_by_size, &mut stop);
                cur.clear();
                long = false;
                idx += 1;
            } else if idx >= start && idx < start + count && !stop {
                if cur.len() < keep { cur.push(b) } else { long = true }
            }
        }
        last_byte = buf[len - 1];
        reader.consume(len);
    }
    let total = if last_byte == b'\n' { idx - 1 } else { finish(idx, &cur, long, &mut out, &mut last, &mut cut_by_size, &mut stop); idx };
    if total == 0 {
        return Ok(ToolResult::ok(format!("{} 是空文件。", ctx.rel(file))));
    }
    if start > total {
        return Err(format!("{} 只有 {total} 行，第 {start} 行不存在。", ctx.rel(file)));
    }
    if last < total {
        let _ = write!(
            out,
            "（共 {total} 行，{}只显示到第 {last} 行；下次从第 {} 行读）",
            if cut_by_size { "超过 50KB，" } else { "" },
            last + 1
        );
    }
    let _ = write!(out, "\n（文件有 {} MB，太大：只读了这一段；要改用 write_patch，不能用 write_file 整个覆盖）", size.div_ceil(1024 * 1024));
    Ok(ToolResult::ok(out))
}

pub fn find_files(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let pattern = str_arg(input, "pattern").map(str::trim).filter(|p| !p.is_empty()).ok_or("缺 pattern。")?;
    let base = resolve(&ctx.root, str_arg(input, "path").unwrap_or("."))?;
    let glob = globset::GlobBuilder::new(pattern)
        .literal_separator(pattern.contains('/'))
        .build()
        .map_err(|e| format!("pattern 不是合法的 glob：{e}"))?
        .compile_matcher();
    let mut found: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in walker(&base, None).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel_base = entry.path().strip_prefix(&base).unwrap_or(entry.path());
        let hit = glob.is_match(rel_base) || (!pattern.contains('/') && glob.is_match(entry.file_name()));
        if hit {
            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::UNIX_EPOCH);
            found.push((mtime, ctx.rel(entry.path())));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let total = found.len();
    if total == 0 {
        return Ok(ToolResult::ok(format!("没找到匹配 {pattern} 的文件。")));
    }
    let mut text = found.iter().take(FIND_LIMIT).map(|(_, p)| p.as_str()).collect::<Vec<_>>().join("\n");
    if total > FIND_LIMIT {
        let _ = write!(text, "\n（共 {total} 个，只列了最新的 {FIND_LIMIT} 个）");
    }
    Ok(ToolResult::ok(text))
}

pub fn write_file(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let raw = str_arg(input, "path").ok_or("缺 path。")?;
    let content = input.get("content").and_then(Value::as_str).ok_or("缺 content。")?;
    let file = resolve(&ctx.root, raw)?;
    if file.is_dir() {
        return Err(format!("{} 是文件夹。", ctx.rel(&file)));
    }
    let rel = ctx.rel(&file);
    let before = match std::fs::read(&file) {
        Ok(bytes) => {
            match ctx.read_stamp(&file) {
                None => return Err(format!("{rel} 已经有了：先用 read_file 读一遍再整个覆盖（只改几处用 write_patch）。")),
                Some(seen) if seen != super::stamp_of(&bytes) => {
                    return Err(format!("{rel} 在你读过以后变了，再读一遍再覆盖。"))
                }
                Some(_) => {}
            }
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        Err(_) => None,
    };
    super::patch::commit(ctx, vec![(file.clone(), before.clone(), content.to_string())])?;
    ctx.remember_original(&rel, before.as_deref());
    ctx.remember_read(&file, content.as_bytes());
    let diff = file_diff(&rel, before.as_deref(), Some(content));
    let lines = content.lines().count();
    let mut result = ToolResult::ok(match &before {
        None => format!("新建了 {rel}（{lines} 行）。"),
        Some(_) => format!("整个覆盖了 {rel}（{lines} 行，+{} −{}）。", diff.additions.unwrap_or(0), diff.deletions.unwrap_or(0)),
    });
    result.diff = vec![diff];
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::testing::TempProject;
    use super::*;
    use crate::gateway::event::PermissionLevel;
    use serde_json::json;

    #[test]
    fn big_files_are_streamed_and_read_the_same_lines() {
        let p = TempProject::new();
        let body: String = (1..=500).map(|i| format!("line {i}\r\n")).collect::<String>() + "last line without newline";
        p.write("big.txt", &body);
        let ctx = p.ctx(PermissionLevel::Edit);
        for input in [json!({"path": "big.txt"}), json!({"path": "big.txt", "start_line": 498, "line_count": 5}), json!({"path": "big.txt", "start_line": 10, "line_count": 3})] {
            let whole = read_file_within(&ctx, &input, u64::MAX).unwrap().text;
            let streamed = read_file_within(&ctx, &input, 100).unwrap().text;
            assert_eq!(streamed.split("\n（文件有").next().unwrap(), whole, "{input}");
        }
        let tail = read_file_within(&ctx, &json!({"path": "big.txt", "start_line": 501}), 100).unwrap().text;
        assert!(tail.starts_with("   501\tlast line without newline\n"));
        assert!(read_file_within(&ctx, &json!({"path": "big.txt", "start_line": 600}), 100).unwrap_err().contains("只有 501 行"));
        // 流式读的不算「读过」：不能整个覆盖
        let fresh = p.ctx(PermissionLevel::Edit);
        read_file_within(&fresh, &json!({"path": "big.txt"}), 100).unwrap();
        assert!(fresh.read_stamp(&p.dir.join("big.txt")).is_none());
    }

    #[test]
    fn list_respects_gitignore_and_depth() {
        let p = TempProject::new();
        p.write(".gitignore", "dist/\n*.log\n")
            .write("src/a.ts", "")
            .write("src/deep/b.ts", "")
            .write("dist/out.js", "")
            .write("x.log", "")
            .write("README.md", "");
        let ctx = p.ctx(PermissionLevel::Edit);
        let one = list_directory(&ctx, &json!({})).unwrap().text;
        assert!(one.contains("src/") && one.contains("README.md") && one.contains(".gitignore"));
        assert!(!one.contains("dist") && !one.contains("x.log") && !one.contains("a.ts"));
        let two = list_directory(&ctx, &json!({"depth": 3})).unwrap().text;
        assert!(two.contains("  a.ts") && two.contains("    b.ts"));
    }

    #[test]
    fn read_numbers_lines_and_says_where_to_continue() {
        let p = TempProject::new();
        let body: String = (1..=2500).map(|i| format!("line {i}\n")).collect();
        p.write("big.txt", &body).write("bin.dat", "a\0b");
        let ctx = p.ctx(PermissionLevel::Edit);
        let r = read_file(&ctx, &json!({"path":"big.txt"})).unwrap().text;
        assert!(r.starts_with("     1\tline 1\n"));
        assert!(r.contains("  2000\tline 2000\n"));
        assert!(r.contains("共 2500 行") && r.contains("下次从第 2001 行读"));
        let r = read_file(&ctx, &json!({"path":"big.txt","start_line":2499,"line_count":5})).unwrap().text;
        assert_eq!(r, "  2499\tline 2499\n  2500\tline 2500\n");
        assert!(read_file(&ctx, &json!({"path":"bin.dat"})).unwrap().text.contains("二进制"));
        assert!(read_file(&ctx, &json!({"path":"nope.txt"})).is_err());
        // 50KB 上限
        let wide: String = (1..=600).map(|i| format!("{i} {}\n", "x".repeat(200))).collect();
        p.write("wide.txt", &wide);
        let r = read_file(&ctx, &json!({"path":"wide.txt"})).unwrap().text;
        assert!(r.len() < READ_BYTES + 200 && r.contains("超过 50KB"));
    }

    #[test]
    fn find_sorts_by_newest() {
        let p = TempProject::new();
        p.write("a/old.rs", "").write("b/new.rs", "").write("c.txt", "");
        let old = std::fs::File::options().write(true).open(p.dir.join("a/old.rs")).unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
        let ctx = p.ctx(PermissionLevel::Edit);
        let r = find_files(&ctx, &json!({"pattern":"*.rs"})).unwrap().text;
        assert_eq!(r, "b/new.rs\na/old.rs");
        let r = find_files(&ctx, &json!({"pattern":"b/**/*.rs"})).unwrap().text;
        assert_eq!(r, "b/new.rs");
        assert!(find_files(&ctx, &json!({"pattern":"*.go"})).unwrap().text.contains("没找到"));
    }

    #[test]
    fn overwriting_needs_a_fresh_read() {
        let p = TempProject::new();
        p.write("a.txt", "one\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let refused = write_file(&ctx, &json!({"path":"a.txt","content":"two\n"})).unwrap_err();
        assert!(refused.contains("先用 read_file"));
        assert_eq!(p.read("a.txt"), "one\n");
        read_file(&ctx, &json!({"path":"a.txt"})).unwrap();
        p.write("a.txt", "changed by someone\n");
        assert!(write_file(&ctx, &json!({"path":"a.txt","content":"two\n"})).unwrap_err().contains("变了"));
        read_file(&ctx, &json!({"path":"a.txt"})).unwrap();
        let ok = write_file(&ctx, &json!({"path":"a.txt","content":"two\n"})).unwrap();
        assert_eq!(p.read("a.txt"), "two\n");
        assert_eq!(ok.diff[0].additions, Some(1));
        // 刚写过的可以接着覆盖，新文件不用读
        write_file(&ctx, &json!({"path":"a.txt","content":"three\n"})).unwrap();
        let created = write_file(&ctx, &json!({"path":"new/b.txt","content":"hi\n"})).unwrap();
        assert!(created.text.contains("新建"));
        assert_eq!(p.read("new/b.txt"), "hi\n");
        assert_eq!(ctx.diff_summary().len(), 2);
    }
}
