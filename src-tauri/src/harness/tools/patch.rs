//! `write_patch`：「找原文 → 换新文」。整批先校验、再一起写（临时文件 + 改名），任何一处不行就一个都不写。
//!
//! 找不到原文时给最像的几行和空白差异；找到多处时列出行号。可以宽松匹配（行尾空白、中英文标点），
//! 但必须仍然唯一，并在结果里写明。

use super::{file_diff, resolve, str_arg, ToolContext, ToolResult};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::PathBuf;

pub const MAX_EDITS: usize = 50;

/// 宽松比较时当成一样的标点（全角 → 半角）。
fn loose_char(c: char) -> Option<char> {
    Some(match c {
        '\r' => return None,
        '，' => ',',
        '。' => '.',
        '：' => ':',
        '；' => ';',
        '（' => '(',
        '）' => ')',
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        '！' => '!',
        '？' => '?',
        '【' => '[',
        '】' => ']',
        '\u{3000}' => ' ',
        other => other,
    })
}

/// 宽松化后的文字，和每个字在原文里的字节范围。
struct Loose {
    text: String,
    /// 宽松文字里每个字的起点（字节）。
    starts: Vec<usize>,
    /// 每个字在原文里的 [起, 止)。
    spans: Vec<(usize, usize)>,
}

fn loosen(original: &str) -> Loose {
    let chars: Vec<(usize, char)> = original.char_indices().collect();
    let mut out = Loose {
        text: String::with_capacity(original.len()),
        starts: Vec::new(),
        spans: Vec::new(),
    };
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        // 行尾空白（到换行或结尾为止都是空格、tab）丢掉。
        if c == ' ' || c == '\t' || c == '\u{3000}' {
            let mut j = i;
            while j < chars.len() && matches!(chars[j].1, ' ' | '\t' | '\u{3000}' | '\r') {
                j += 1;
            }
            if j == chars.len() || chars[j].1 == '\n' {
                i = j;
                continue;
            }
        }
        if let Some(mapped) = loose_char(c) {
            out.starts.push(out.text.len());
            out.text.push(mapped);
            out.spans.push((at, at + c.len_utf8()));
        }
        i += 1;
    }
    out
}

impl Loose {
    /// 宽松文字里的一段 [pos, pos+len) → 原文里的字节范围。
    fn original_range(&self, pos: usize, len: usize) -> (usize, usize) {
        let first = self.starts.binary_search(&pos).unwrap_or_else(|i| i);
        let end = self.starts.binary_search(&(pos + len)).unwrap_or_else(|i| i);
        (self.spans[first].0, self.spans[end.saturating_sub(1)].1)
    }
}

fn line_of(text: &str, byte: usize) -> usize {
    text[..byte].matches('\n').count() + 1
}

fn exact_ranges(content: &str, old: &str) -> Vec<(usize, usize)> {
    content.match_indices(old).map(|(i, m)| (i, i + m.len())).collect()
}

fn loose_ranges(content: &str, old: &str) -> Vec<(usize, usize)> {
    let needle = loosen(old).text;
    if needle.trim().is_empty() {
        return vec![];
    }
    let hay = loosen(content);
    hay.text
        .match_indices(&needle)
        .map(|(pos, m)| hay.original_range(pos, m.len()))
        .collect()
}

/// 看得见的空白：tab 写成 →，空格写成 ·。
fn visible(s: &str) -> String {
    s.replace('\t', "→").replace(' ', "·")
}

/// 找不到原文时：最像的几行。
fn near_misses(content: &str, old: &str) -> String {
    let Some(probe) = old.lines().map(str::trim_end).find(|l| !l.trim().is_empty()) else {
        return String::new();
    };
    let mut scored: Vec<(f32, usize, &str)> = content
        .lines()
        .enumerate()
        .take(20_000)
        .filter(|(_, l)| l.len() < 1000 && !l.trim().is_empty())
        .map(|(i, l)| (similar::TextDiff::from_chars(probe.trim(), l.trim()).ratio(), i + 1, l))
        .filter(|(r, _, _)| *r >= 0.5)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let mut out = String::new();
    for (_, n, line) in scored.into_iter().take(3) {
        let _ = write!(out, "\n  第 {n} 行：{}", line.trim_end());
        if line.trim() == probe.trim() && line.trim_end() != probe {
            let lead = |s: &str| s[..s.len() - s.trim_start().len()].to_string();
            let _ = write!(out, "\n    （只差空白：文件里是「{}」，你给的是「{}」）", visible(&lead(line)), visible(&lead(probe)));
        }
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("\n最像的几行：{out}")
    }
}

/// 改动处前后 3 行（带行号）。
fn excerpt(content: &str, start: usize, end: usize) -> String {
    let first = line_of(content, start);
    let last = line_of(content, end.max(start));
    let lines: Vec<&str> = content.lines().collect();
    let from = first.saturating_sub(3).max(1);
    let to = (last + 3).min(lines.len().max(1));
    let mut out = String::new();
    for n in from..=to {
        if let Some(l) = lines.get(n - 1) {
            let _ = writeln!(out, "{n:>6}\t{l}");
        }
    }
    out
}

pub fn write_patch(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let edits = input.get("edits").and_then(Value::as_array).ok_or("缺 edits（一组 {path, old_text, new_text}）。")?;
    if edits.is_empty() {
        return Err("edits 是空的。".into());
    }
    if edits.len() > MAX_EDITS {
        return Err(format!("一次最多 {MAX_EDITS} 处，分几批改。"));
    }
    // 每个文件：路径、原来的内容、改到现在的内容。
    let mut staged: Vec<(PathBuf, String, String, String)> = Vec::new();
    let mut report = Vec::new();
    for (i, edit) in edits.iter().enumerate() {
        let n = i + 1;
        let raw = str_arg(edit, "path").ok_or(format!("第 {n} 处缺 path。"))?;
        let old = edit.get("old_text").and_then(Value::as_str).ok_or(format!("第 {n} 处缺 old_text。"))?;
        let new = edit.get("new_text").and_then(Value::as_str).ok_or(format!("第 {n} 处缺 new_text。"))?;
        let replace_all = edit.get("replace_all").and_then(Value::as_bool).unwrap_or(false);
        let file = resolve(&ctx.root, raw)?;
        let rel = ctx.rel(&file);
        if old.is_empty() {
            return Err(format!("第 {n} 处（{rel}）：old_text 不能为空；新建文件用 write_file。一处都没改。"));
        }
        let idx = match staged.iter().position(|(p, ..)| *p == file) {
            Some(idx) => idx,
            None => {
                let bytes = std::fs::read(&file).map_err(|_| format!("第 {n} 处：{rel} 不存在（新建用 write_file）。一处都没改。"))?;
                let text = String::from_utf8(bytes).map_err(|_| format!("第 {n} 处：{rel} 不是 UTF-8 文本。一处都没改。"))?;
                staged.push((file.clone(), rel.clone(), text.clone(), text));
                staged.len() - 1
            }
        };
        let current = staged[idx].3.clone();
        let mut ranges = exact_ranges(&current, old);
        let mut loose = false;
        if ranges.is_empty() {
            ranges = loose_ranges(&current, old);
            loose = !ranges.is_empty();
        }
        if ranges.is_empty() {
            return Err(format!(
                "第 {n} 处：在 {rel} 里找不到 old_text。一处都没改。{}",
                near_misses(&current, old)
            ));
        }
        if ranges.len() > 1 && !replace_all {
            let lines: Vec<String> = ranges.iter().map(|(s, _)| line_of(&current, *s).to_string()).collect();
            return Err(format!(
                "第 {n} 处：old_text 在 {rel} 里有 {} 处（第 {} 行）{}。多带几行让它唯一，或者设 replace_all。一处都没改。",
                ranges.len(),
                lines.join("、"),
                if loose { "（宽松匹配）" } else { "" }
            ));
        }
        let mut next = current.clone();
        for (s, e) in ranges.iter().rev() {
            next.replace_range(*s..*e, new);
        }
        let at = ranges[0].0;
        let shown = excerpt(&next, at, at + new.trim_end_matches('\n').len());
        let mut line = format!(
            "{rel} 第 {} 行{}{}：\n{shown}",
            line_of(&current, at),
            if ranges.len() > 1 { format!("等 {} 处", ranges.len()) } else { String::new() },
            if loose { "（宽松匹配：忽略了行尾空白、中英文标点的差别）" } else { "" }
        );
        if line.ends_with('\n') {
            line.pop();
        }
        report.push(line);
        staged[idx].3 = next;
    }
    let files: Vec<(PathBuf, Option<String>, String)> = staged
        .iter()
        .filter(|(_, _, before, after)| before != after)
        .map(|(p, _, before, after)| (p.clone(), Some(before.clone()), after.clone()))
        .collect();
    commit(ctx, files)?;
    let mut diffs = Vec::new();
    for (path, rel, before, after) in &staged {
        ctx.remember_original(rel, Some(before));
        ctx.remember_read(path, after.as_bytes());
        if before != after {
            diffs.push(file_diff(rel, Some(before), Some(after)));
        }
    }
    let mut result = ToolResult::ok(format!("改好了 {} 处：\n\n{}", edits.len(), report.join("\n\n")));
    result.diff = diffs;
    Ok(result)
}

#[cfg(test)]
thread_local! {
    /// 测试用：第几个文件改名时假装失败。
    pub(crate) static FAIL_RENAME_AT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// 一起写：先全部写成临时文件，再一个个改名；改名中途失败就把已经换掉的写回原样。
pub fn commit(ctx: &ToolContext, files: Vec<(PathBuf, Option<String>, String)>) -> Result<(), String> {
    let mut temps: Vec<PathBuf> = Vec::new();
    let cleanup = |temps: &[PathBuf]| {
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
    };
    for (path, _, content) in &files {
        let dir = path.parent().ok_or("路径不对。")?;
        if let Err(e) = std::fs::create_dir_all(dir) {
            cleanup(&temps);
            return Err(format!("建不了文件夹 {}：{e}。一处都没改。", ctx.rel(dir)));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let temp = dir.join(format!(".{name}.peck-{}.tmp", uuid::Uuid::new_v4().simple()));
        let written = std::fs::write(&temp, content).and_then(|_| match std::fs::metadata(path) {
            Ok(meta) => std::fs::set_permissions(&temp, meta.permissions()),
            Err(_) => Ok(()),
        });
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temp);
            cleanup(&temps);
            return Err(format!("写不进 {}：{e}。一处都没改。", ctx.rel(path)));
        }
        temps.push(temp);
    }
    for (i, ((path, _, _), temp)) in files.iter().zip(&temps).enumerate() {
        #[cfg(test)]
        let forced = FAIL_RENAME_AT.with(|f| f.get()) == Some(i);
        #[cfg(not(test))]
        let forced = false;
        let renamed = if forced {
            Err(std::io::Error::other("测试里假装失败"))
        } else {
            std::fs::rename(temp, path)
        };
        if let Err(e) = renamed {
            // 已经换掉的写回原样，剩下的临时文件删掉；写不回去的照实说。
            let mut stuck: Vec<String> = Vec::new();
            for (done, prior, _) in &files[..i] {
                let restored = match prior {
                    Some(text) => std::fs::write(done, text),
                    None => std::fs::remove_file(done),
                };
                if restored.is_err() {
                    stuck.push(ctx.rel(done));
                }
            }
            cleanup(&temps[i..]);
            if !stuck.is_empty() {
                return Err(format!(
                    "换不了 {}：{e}。这几个文件已经改了、但没能写回原样，请先看一下：{}。",
                    ctx.rel(path),
                    stuck.join("、")
                ));
            }
            return Err(format!("换不了 {}：{e}。已经写回原样，一处都没改。", ctx.rel(path)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testing::TempProject;
    use super::*;
    use crate::gateway::event::PermissionLevel;
    use serde_json::json;

    fn edit(path: &str, old: &str, new: &str) -> Value {
        json!({"path": path, "old_text": old, "new_text": new})
    }

    #[test]
    fn a_batch_is_all_or_nothing() {
        let p = TempProject::new();
        p.write("a.txt", "alpha\nbeta\n").write("b.txt", "one\ntwo\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let err = write_patch(&ctx, &json!({"edits":[edit("a.txt","alpha","ALPHA"), edit("b.txt","three","3")]})).unwrap_err();
        assert!(err.contains("第 2 处") && err.contains("一处都没改"));
        assert_eq!(p.read("a.txt"), "alpha\nbeta\n");
        let ok = write_patch(&ctx, &json!({"edits":[edit("a.txt","alpha","ALPHA"), edit("b.txt","two","2"), edit("a.txt","beta","BETA")]})).unwrap();
        assert_eq!(p.read("a.txt"), "ALPHA\nBETA\n");
        assert_eq!(p.read("b.txt"), "one\n2\n");
        assert_eq!(ok.diff.len(), 2);
        assert!(ok.text.contains("a.txt 第 1 行"));
        // 没留下临时文件
        assert!(std::fs::read_dir(&p.dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("peck-")));
    }

    #[test]
    fn a_failed_rename_rolls_everything_back() {
        let p = TempProject::new();
        p.write("a.txt", "a\n").write("b.txt", "b\n").write("c.txt", "c\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        FAIL_RENAME_AT.with(|f| f.set(Some(2)));
        let err = write_patch(&ctx, &json!({"edits":[edit("a.txt","a","A"), edit("b.txt","b","B"), edit("c.txt","c","C")]})).unwrap_err();
        FAIL_RENAME_AT.with(|f| f.set(None));
        assert!(err.contains("写回原样"));
        assert_eq!((p.read("a.txt"), p.read("b.txt"), p.read("c.txt")), ("a\n".into(), "b\n".into(), "c\n".into()));
        assert!(std::fs::read_dir(&p.dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("peck-")));
    }

    #[test]
    fn many_matches_list_their_lines_unless_replace_all() {
        let p = TempProject::new();
        p.write("a.txt", "x = 1\ny = 2\nx = 1\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let err = write_patch(&ctx, &json!({"edits":[edit("a.txt","x = 1","x = 9")]})).unwrap_err();
        assert!(err.contains("2 处") && err.contains("第 1、3 行"));
        let all = json!({"edits":[{"path":"a.txt","old_text":"x = 1","new_text":"x = 9","replace_all":true}]});
        assert!(write_patch(&ctx, &all).unwrap().text.contains("等 2 处"));
        assert_eq!(p.read("a.txt"), "x = 9\ny = 2\nx = 9\n");
    }

    #[test]
    fn loose_matching_is_marked_and_still_unique() {
        let p = TempProject::new();
        p.write("a.md", "标题：待办   \n说明（旧）\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let r = write_patch(&ctx, &json!({"edits":[edit("a.md","标题:待办\n说明(旧)","标题：待办\n说明（新）")]})).unwrap();
        assert!(r.text.contains("宽松匹配"));
        assert_eq!(p.read("a.md"), "标题：待办\n说明（新）\n");
        p.write("b.md", "a，b\na，b  \n");
        let err = write_patch(&ctx, &json!({"edits":[edit("b.md","a,b","x")]})).unwrap_err();
        assert!(err.contains("2 处") && err.contains("宽松匹配"), "宽松以后不唯一也不改：{err}");
        assert_eq!(p.read("b.md"), "a，b\na，b  \n");
        p.write("c.md", "a，b\na,b\n");
        let err = write_patch(&ctx, &json!({"edits":[edit("c.md","a；b","x")]})).unwrap_err();
        assert!(err.contains("找不到"));
    }

    #[test]
    fn misses_show_the_closest_lines_and_whitespace() {
        let p = TempProject::new();
        p.write("a.py", "def main():\n\treturn 1\n");
        let ctx = p.ctx(PermissionLevel::Edit);
        let err = write_patch(&ctx, &json!({"edits":[edit("a.py","    return 1\n    pass","x")]})).unwrap_err();
        assert!(err.contains("第 2 行"), "{err}");
        assert!(err.contains("只差空白") && err.contains("→"), "{err}");
        assert!(write_patch(&ctx, &json!({"edits":[edit("nope.py","a","b")]})).unwrap_err().contains("不存在"));
        assert!(write_patch(&ctx, &json!({"edits":[edit("a.py","","b")]})).unwrap_err().contains("不能为空"));
    }

    #[test]
    fn results_show_three_lines_around() {
        let p = TempProject::new();
        let body: String = (1..=20).map(|i| format!("{i}\n")).collect();
        p.write("n.txt", &body);
        let ctx = p.ctx(PermissionLevel::Edit);
        let r = write_patch(&ctx, &json!({"edits":[edit("n.txt","10\n","ten\n")]})).unwrap();
        assert!(r.text.contains("     7\t7") && r.text.contains("    10\tten") && r.text.contains("    13\t13"));
        assert!(!r.text.contains("     6\t6") && !r.text.contains("    14\t14"));
    }
}
