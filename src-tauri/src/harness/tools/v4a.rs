//! `apply_patch`（v0.8）：OpenAI 的 V4A 补丁格式。GPT 这一家的模型（OpenAI 的线协议）训练时改文件就用它，
//! 给它们这个工具代替 `write_patch`。格式：
//!
//! ```text
//! *** Begin Patch
//! *** Add File: path/new.txt
//! +第一行
//! *** Update File: src/app.ts
//! *** Move to: src/main.ts        （可选：改名）
//! @@ function render() {           （可选：先找到这一行，再在它后面找下面这段）
//!  原样的一行
//! -删掉的一行
//! +加上的一行
//! *** End of File                  （可选：这一段在文件末尾）
//! *** Delete File: old.txt
//! *** End Patch
//! ```
//!
//! 整份先在内存里算好、校验完再一起写（`patch::commit`：临时文件 + 改名，中途失败写回原样）；找不到上下文
//! 一处都不改，报是哪个文件的哪一段。匹配先精确、再忽略行尾空白、再忽略两头空白。删文件要站主批（循环里问）。

use super::{file_diff, patch, ToolContext, ToolResult};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Add { path: String, content: String },
    Delete { path: String },
    Update { path: String, move_to: Option<String>, chunks: Vec<Chunk> },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Chunk {
    /// `@@ ` 后面的那一行：先找到它，这一段在它后面找。
    pub anchor: Option<String>,
    /// 原来的几行（上下文 + 删掉的），按顺序。
    pub old: Vec<String>,
    /// 改成的几行（上下文 + 加上的）。
    pub new: Vec<String>,
    /// 这一段在文件末尾。
    pub eof: bool,
}

/// 补丁的文字：参数叫 `patch` 或 `input`（OpenAI 自己的工具叫 input）。
pub fn patch_text(input: &Value) -> Option<&str> {
    input.get("patch").or_else(|| input.get("input")).and_then(Value::as_str)
}

pub fn parse(text: &str) -> Result<Vec<Op>, String> {
    let mut lines: Vec<&str> = text.lines().collect();
    // 包在 ``` 里的、前后有空行的都认。
    while lines.first().is_some_and(|l| l.trim().is_empty() || l.trim_start().starts_with("```")) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty() || l.trim_start().starts_with("```")) {
        lines.pop();
    }
    if lines.first().map(|l| l.trim()) != Some("*** Begin Patch") {
        return Err("补丁要以「*** Begin Patch」开头。".into());
    }
    if lines.last().map(|l| l.trim()) != Some("*** End Patch") {
        return Err("补丁要以「*** End Patch」结尾。".into());
    }
    let body = &lines[1..lines.len() - 1];
    let mut ops = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            i += 1;
            let mut content = String::new();
            while i < body.len() && !body[i].starts_with("*** ") {
                let l = body[i];
                let Some(rest) = l.strip_prefix('+') else {
                    return Err(format!("Add File {path}：每一行都要以 + 开头（第 {} 行不是）。", i + 2));
                };
                content.push_str(rest);
                content.push('\n');
                i += 1;
            }
            ops.push(Op::Add { path: path.trim().to_string(), content });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(Op::Delete { path: path.trim().to_string() });
            i += 1;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            i += 1;
            let mut move_to = None;
            if let Some(to) = body.get(i).and_then(|l| l.strip_prefix("*** Move to: ")) {
                move_to = Some(to.trim().to_string());
                i += 1;
            }
            let mut chunks: Vec<Chunk> = Vec::new();
            let mut cur: Option<Chunk> = None;
            while i < body.len() {
                let l = body[i];
                if l.starts_with("*** ") && l.trim() != "*** End of File" {
                    break;
                }
                if l.trim() == "*** End of File" {
                    cur.get_or_insert_with(Chunk::default).eof = true;
                    i += 1;
                    continue;
                }
                if let Some(rest) = l.strip_prefix("@@") {
                    if let Some(c) = cur.take() {
                        chunks.push(c);
                    }
                    let anchor = rest.trim();
                    cur = Some(Chunk { anchor: (!anchor.is_empty()).then(|| anchor.to_string()), ..Default::default() });
                    i += 1;
                    continue;
                }
                let c = cur.get_or_insert_with(Chunk::default);
                if c.eof {
                    return Err(format!("Update File {path}：*** End of File 后面不能再有改动。"));
                }
                match l.chars().next() {
                    Some('+') => c.new.push(l[1..].to_string()),
                    Some('-') => c.old.push(l[1..].to_string()),
                    Some(' ') => {
                        c.old.push(l[1..].to_string());
                        c.new.push(l[1..].to_string());
                    }
                    None => {
                        c.old.push(String::new());
                        c.new.push(String::new());
                    }
                    _ => return Err(format!("Update File {path}：「{l}」要以空格、+、- 或 @@ 开头。")),
                }
                i += 1;
            }
            if let Some(c) = cur.take() {
                chunks.push(c);
            }
            if chunks.is_empty() && move_to.is_none() {
                return Err(format!("Update File {path}：没有要改的。"));
            }
            ops.push(Op::Update { path: path.trim().to_string(), move_to, chunks });
        } else {
            return Err(format!("看不懂这一行：「{line}」（要 *** Add File / Update File / Delete File）。"));
        }
    }
    if ops.is_empty() {
        return Err("补丁里没有要改的文件。".into());
    }
    Ok(ops)
}

/// 补丁要删哪些文件（循环里先问站主）。
pub fn deletes(input: &Value) -> Vec<String> {
    patch_text(input)
        .and_then(|t| parse(t).ok())
        .map(|ops| {
            ops.into_iter()
                .filter_map(|o| match o {
                    Op::Delete { path } => Some(path),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 在 `lines[from..]` 里找 `want` 这一串：先精确，再忽略行尾空白，再忽略两头空白。`eof` 先试文件末尾。
fn seek(lines: &[String], want: &[String], from: usize, eof: bool) -> Option<usize> {
    if want.is_empty() {
        return Some(if eof { lines.len() } else { from.min(lines.len()) });
    }
    if want.len() > lines.len() {
        return None;
    }
    let tests: [fn(&str, &str) -> bool; 3] = [|a, b| a == b, |a, b| a.trim_end() == b.trim_end(), |a, b| a.trim() == b.trim()];
    for same in tests {
        let at = |s: usize| want.iter().enumerate().all(|(k, w)| same(&lines[s + k], w));
        if eof {
            let s = lines.len() - want.len();
            if s >= from && at(s) {
                return Some(s);
            }
        }
        if let Some(s) = (from..=lines.len() - want.len()).find(|&s| at(s)) {
            return Some(s);
        }
    }
    None
}

/// 宽松匹配（忽略空白）命中时，以文件里原来的为准：上下文行照文件原样，新加的行把补丁的缩进换成文件的
/// （模型常把 4 个空格写成 Tab，Phase 13 真跑碰到：不换的话整个文件的缩进被悄悄改掉）。
fn reindent(old: &[String], original: &[String], new: &[String]) -> Vec<String> {
    if old == original {
        return new.to_vec();
    }
    let lead = |s: &str| s[..s.len() - s.trim_start().len()].to_string();
    let shift = old
        .iter()
        .zip(original)
        .find(|(o, f)| !o.trim().is_empty() && lead(o) != lead(f))
        .map(|(o, f)| (lead(o), lead(f)));
    new.iter()
        .map(|n| {
            if let Some(k) = old.iter().position(|o| o == n) {
                return original[k].clone();
            }
            match &shift {
                Some((from, to)) if !from.is_empty() && n.starts_with(from.as_str()) => format!("{to}{}", &n[from.len()..]),
                _ => n.clone(),
            }
        })
        .collect()
}

/// 把一个文件按段改好（内存里）。
fn apply_chunks(path: &str, text: &str, chunks: &[Chunk]) -> Result<String, String> {
    let trailing = text.ends_with('\n');
    // 原来是 CRLF 的照样写 CRLF（`lines()` 去掉了 \r）。
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut cursor = 0usize;
    let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for (n, c) in chunks.iter().enumerate() {
        if let Some(anchor) = &c.anchor {
            let found = seek(&lines, std::slice::from_ref(anchor), cursor, false)
                .ok_or_else(|| format!("{path} 第 {} 段：找不到「@@ {anchor}」这一行。一处都没改。", n + 1))?;
            cursor = found + 1;
        }
        // 只有加的行、又没有 @@ 定位：加在文件末尾（和 Codex 的 apply_patch 一样），不是开头。
        let eof = c.eof || (c.old.is_empty() && c.anchor.is_none());
        let at = seek(&lines, &c.old, cursor, eof).ok_or_else(|| {
            let first = c.old.iter().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
            format!("{path} 第 {} 段：找不到要改的那几行（从「{}」开始）。先 read_file 看看现在的样子。一处都没改。", n + 1, first.trim())
        })?;
        let original = lines[at..at + c.old.len()].to_vec();
        edits.push((at, c.old.len(), reindent(&c.old, &original, &c.new)));
        cursor = at + c.old.len();
    }
    edits.sort_by_key(|e| e.0);
    for (at, len, new) in edits.into_iter().rev() {
        lines.splice(at..at + len, new);
    }
    let mut out = lines.join(eol);
    // 原来以换行结尾（或者是空文件）就照样以换行结尾；原来没有就不加。
    if (trailing || text.is_empty()) && !lines.is_empty() {
        out.push_str(eol);
    }
    Ok(out)
}

pub fn apply_patch(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let text = patch_text(input).ok_or("缺 patch（V4A 格式，*** Begin Patch … *** End Patch）。")?;
    let ops = parse(text)?;
    // 每个文件：绝对路径、显示路径、原来的内容（没有为空）、改成的内容（删掉为空）。
    let mut staged: Vec<(PathBuf, String, Option<String>, Option<String>)> = Vec::new();
    let read = |p: &PathBuf, rel: &str| -> Result<Option<String>, String> {
        match std::fs::read(p) {
            Ok(b) => String::from_utf8(b).map(Some).map_err(|_| format!("{rel} 不是 UTF-8 文本。一处都没改。")),
            Err(_) => Ok(None),
        }
    };
    let mut report = Vec::new();
    for op in &ops {
        match op {
            Op::Add { path, content } => {
                let file = ctx.resolve_path(path, true)?;
                let rel = ctx.rel(&file);
                if staged.iter().any(|(p, ..)| *p == file) || file.exists() {
                    return Err(format!("{rel} 已经有了：改它用 *** Update File。一处都没改。"));
                }
                report.push(format!("新建 {rel}（{} 行）", content.lines().count()));
                staged.push((file, rel, None, Some(content.clone())));
            }
            Op::Delete { path } => {
                let file = ctx.resolve_path(path, true)?;
                let rel = ctx.rel(&file);
                let before = read(&file, &rel)?.ok_or_else(|| format!("{rel} 不存在，删不了。一处都没改。"))?;
                report.push(format!("删掉 {rel}"));
                staged.push((file, rel, Some(before), None));
            }
            Op::Update { path, move_to, chunks } => {
                let file = ctx.resolve_path(path, true)?;
                let rel = ctx.rel(&file);
                let (before, current) = match staged.iter().position(|(p, ..)| *p == file) {
                    Some(i) => (staged[i].2.clone(), staged[i].3.clone()),
                    None => {
                        let b = read(&file, &rel)?;
                        (b.clone(), b)
                    }
                };
                let current = current.ok_or_else(|| format!("{rel} 不存在（新建用 *** Add File）。一处都没改。"))?;
                let next = apply_chunks(&rel, &current, chunks)?;
                staged.retain(|(p, ..)| *p != file);
                // 改名成自己（Move to 同一个路径）就是不改名：不然先写后删，文件就没了。
                let target = match move_to {
                    Some(to) => Some(ctx.resolve_path(to, true)?).filter(|t| *t != file),
                    None => None,
                };
                match target {
                    Some(target) => {
                        let trel = ctx.rel(&target);
                        if target.exists() || staged.iter().any(|(p, ..)| *p == target) {
                            return Err(format!("{trel} 已经有了，不能改名过去。一处都没改。"));
                        }
                        report.push(format!("改 {rel} 并改名成 {trel}"));
                        staged.push((file, rel, before, None));
                        staged.push((target, trel, None, Some(next)));
                    }
                    None => {
                        report.push(format!("改 {rel}（{} 段）", chunks.len()));
                        staged.push((file, rel, before, Some(next)));
                    }
                }
            }
        }
    }
    let writes: Vec<(PathBuf, Option<String>, String)> = staged
        .iter()
        .filter_map(|(p, _, before, after)| after.as_ref().filter(|a| Some(*a) != before.as_ref()).map(|a| (p.clone(), before.clone(), a.clone())))
        .collect();
    patch::commit(ctx, writes)?;
    for (p, rel, _, after) in &staged {
        if after.is_none() {
            std::fs::remove_file(p).map_err(|e| format!("删不掉 {rel}：{e}（前面的改动已经写了）。"))?;
        }
    }
    let mut diffs = Vec::new();
    for (p, rel, before, after) in &staged {
        ctx.remember_original(rel, before.as_deref());
        if let Some(a) = after {
            ctx.remember_read(p, a.as_bytes());
        }
        if before != after {
            diffs.push(file_diff(rel, before.as_deref(), after.as_deref()));
        }
    }
    let mut result = ToolResult::ok(format!("补丁打好了：\n{}", report.join("\n")));
    result.diff = diffs;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::testing::TempProject;
    use super::*;
    use crate::gateway::event::PermissionLevel;
    use serde_json::json;

    #[test]
    fn adds_updates_moves_and_deletes_in_one_patch() {
        let p = TempProject::new();
        p.write("src/app.ts", "function render() {\n  return 1;\n}\n\nfunction other() {\n  return 1;\n}\n").write("old.txt", "x\n").write("a.md", "a\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let patch = "*** Begin Patch\n*** Add File: notes/new.txt\n+你好\n+world\n*** Update File: src/app.ts\n@@ function other() {\n-  return 1;\n+  return 2;\n*** Update File: a.md\n*** Move to: b.md\n@@\n-a\n+b\n*** Delete File: old.txt\n*** End Patch\n";
        let r = apply_patch(&ctx, &json!({ "patch": patch })).unwrap();
        assert!(r.ok, "{}", r.text);
        assert_eq!(p.read("notes/new.txt"), "你好\nworld\n");
        // 第一个 return 1 不动：@@ 先找到 other() 再往下找。
        assert_eq!(p.read("src/app.ts"), "function render() {\n  return 1;\n}\n\nfunction other() {\n  return 2;\n}\n");
        assert_eq!(p.read("b.md"), "b\n");
        assert!(!p.dir.join("a.md").exists() && !p.dir.join("old.txt").exists());
        assert_eq!(r.diff.len(), 5);
        assert_eq!(deletes(&json!({ "input": patch })), vec!["old.txt".to_string()]);
    }

    #[test]
    fn loose_matches_keep_the_files_own_indentation() {
        let chunk = Chunk { anchor: None, old: vec!["def f():".into(), "\treturn 1".into()], new: vec!["def f():".into(), "\treturn 2".into(), "\t# done".into()], eof: false };
        let out = apply_chunks("a.py", "def f():\n    return 1\n", &[chunk]).unwrap();
        assert_eq!(out, "def f():\n    return 2\n    # done\n");
        // 精确匹配的照补丁写。
        let exact = Chunk { anchor: None, old: vec!["\tx".into()], new: vec!["\ty".into()], eof: false };
        assert_eq!(apply_chunks("a.go", "\tx\n", &[exact]).unwrap(), "\ty\n");
    }

    #[test]
    fn a_missing_context_changes_nothing_and_whitespace_is_forgiven() {
        let p = TempProject::new();
        p.write("a.txt", "one\ntwo  \nthree\n").write("b.txt", "keep\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let bad = "*** Begin Patch\n*** Update File: b.txt\n-keep\n+changed\n*** Update File: a.txt\n-nope\n+x\n*** End Patch";
        let err = apply_patch(&ctx, &json!({ "patch": bad })).unwrap_err();
        assert!(err.contains("a.txt") && err.contains("一处都没改"), "{err}");
        assert_eq!(p.read("b.txt"), "keep\n");
        let ok = "*** Begin Patch\n*** Update File: a.txt\n one\n-two\n+TWO\n three\n*** End Patch";
        apply_patch(&ctx, &json!({ "patch": ok })).unwrap();
        assert_eq!(p.read("a.txt"), "one\nTWO\nthree\n");
        // 格式不对、碰 .git、出项目都拒掉。
        assert!(parse("*** Update File: a\n").is_err());
        assert!(apply_patch(&ctx, &json!({ "patch": "*** Begin Patch\n*** Add File: .git/x\n+1\n*** End Patch" })).is_err());
        assert!(apply_patch(&ctx, &json!({ "patch": "*** Begin Patch\n*** Add File: ../x\n+1\n*** End Patch" })).is_err());
        assert!(apply_patch(&ctx, &json!({ "patch": "*** Begin Patch\n*** Add File: a.txt\n+1\n*** End Patch" })).is_err());
    }

    #[test]
    fn move_to_itself_keeps_the_file_crlf_stays_and_bare_additions_append() {
        let p = TempProject::new();
        p.write("a.txt", "a\n").write("w.txt", "x\r\ny\r\nz\r\n").write("e.txt", "first\nlast\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let same = "*** Begin Patch\n*** Update File: a.txt\n*** Move to: a.txt\n@@\n-a\n+b\n*** End Patch";
        assert!(apply_patch(&ctx, &json!({ "patch": same })).unwrap().ok);
        assert_eq!(p.read("a.txt"), "b\n");
        apply_patch(&ctx, &json!({ "patch": "*** Begin Patch\n*** Update File: w.txt\n x\n-y\n+Y\n*** End Patch" })).unwrap();
        assert_eq!(p.read("w.txt"), "x\r\nY\r\nz\r\n");
        apply_patch(&ctx, &json!({ "patch": "*** Begin Patch\n*** Update File: e.txt\n+appended\n*** End Patch" })).unwrap();
        assert_eq!(p.read("e.txt"), "first\nlast\nappended\n");
    }
}
