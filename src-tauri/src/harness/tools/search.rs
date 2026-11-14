//! `search_text`：进程内用 ripgrep 的库（`ignore` 遍历、`grep-searcher` 搜），遵守 .gitignore。

use super::fs::walker;
use super::{bool_arg, str_arg, u64_arg, ToolContext, ToolResult};
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkContextKind, SinkMatch};
use serde_json::Value;
use std::fmt::Write as _;

pub const DEFAULT_MAX: usize = 100;
const LINE_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Content,
    Files,
    Count,
}

/// 一个文件里的结果行：(行号, 是不是命中行, 内容)；`None` 是两组上下文之间的断开。
struct Collect {
    rows: Vec<Option<(u64, bool, String)>>,
    matches: usize,
}

fn text_of(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim_end_matches(['\n', '\r']);
    if s.chars().count() > LINE_CHARS {
        format!("{}…", s.chars().take(LINE_CHARS).collect::<String>())
    } else {
        s.to_string()
    }
}

impl Sink for Collect {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        self.matches += 1;
        self.rows.push(Some((mat.line_number().unwrap_or(0), true, text_of(mat.bytes()))));
        Ok(true)
    }

    fn context(&mut self, _: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, Self::Error> {
        if !matches!(ctx.kind(), SinkContextKind::Other) {
            self.rows.push(Some((ctx.line_number().unwrap_or(0), false, text_of(ctx.bytes()))));
        }
        Ok(true)
    }

    fn context_break(&mut self, _: &Searcher) -> Result<bool, Self::Error> {
        self.rows.push(None);
        Ok(true)
    }
}

pub fn search_text(ctx: &ToolContext, input: &Value) -> Result<ToolResult, String> {
    let pattern = str_arg(input, "pattern").filter(|p| !p.is_empty()).ok_or("缺 pattern。")?;
    let base = ctx.resolve_path(str_arg(input, "path").unwrap_or("."), false)?;
    if !base.exists() {
        return Err(format!("{} 不存在。", ctx.rel(&base)));
    }
    let regex = bool_arg(input, "regex").unwrap_or(false);
    let mode = match str_arg(input, "mode").unwrap_or("content") {
        "files" | "files_with_matches" => Mode::Files,
        "count" => Mode::Count,
        _ => Mode::Content,
    };
    let context = u64_arg(input, "context").unwrap_or(0).min(10) as usize;
    let max = u64_arg(input, "max").unwrap_or(DEFAULT_MAX as u64).clamp(1, 500) as usize;
    let offset = u64_arg(input, "offset").unwrap_or(0) as usize;
    let matcher = RegexMatcherBuilder::new()
        .fixed_strings(!regex)
        .build(pattern)
        .map_err(|e| format!("pattern 不是合法的正则：{e}"))?;
    let glob = match str_arg(input, "glob").map(str::trim).filter(|g| !g.is_empty()) {
        Some(g) => Some(
            globset::GlobBuilder::new(g)
                .literal_separator(g.contains('/'))
                .build()
                .map_err(|e| format!("glob 不对：{e}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0))
        .before_context(if mode == Mode::Content { context } else { 0 })
        .after_context(if mode == Mode::Content { context } else { 0 })
        .build();

    // 结果一条一条排好（content：一行；files：一个文件；count：一个文件），再按 offset、max 截。
    let mut results: Vec<String> = Vec::new();
    let mut files_hit = 0usize;
    let mut total_matches = 0usize;
    let stop_at = offset + max;
    let mut more = false;
    for entry in walker(&base, None).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Some(g) = &glob {
            let rel = entry.path().strip_prefix(&ctx.root).unwrap_or(entry.path());
            if !(g.is_match(rel) || g.is_match(entry.file_name())) {
                continue;
            }
        }
        if entry.metadata().map(|m| m.len() > 8 * 1024 * 1024).unwrap_or(true) {
            continue;
        }
        let mut sink = Collect { rows: Vec::new(), matches: 0 };
        if searcher.search_path(&matcher, entry.path(), &mut sink).is_err() || sink.matches == 0 {
            continue;
        }
        files_hit += 1;
        total_matches += sink.matches;
        let rel = ctx.rel(entry.path());
        match mode {
            Mode::Files => results.push(rel),
            Mode::Count => results.push(format!("{rel}:{}", sink.matches)),
            Mode::Content => {
                // 带上下文时，两个文件之间也隔开。
                if context > 0 && results.last().is_some_and(|l| l != "--") {
                    results.push("--".into());
                }
                for row in sink.rows {
                    match row {
                        Some((n, true, text)) => results.push(format!("{rel}:{n}: {text}")),
                        Some((n, false, text)) => results.push(format!("{rel}-{n}- {text}")),
                        None => results.push("--".into()),
                    }
                }
            }
        }
        if results.len() > stop_at + 50 {
            more = true;
            break;
        }
    }
    if results.is_empty() {
        return Ok(ToolResult::ok(format!("没搜到「{pattern}」。")));
    }
    let shown: Vec<&String> = results.iter().skip(offset).take(max).collect();
    let mut text = shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    let left = results.len().saturating_sub(offset + shown.len());
    if left > 0 || more {
        let _ = write!(
            text,
            "\n（还有{}结果没列；下次 offset 设 {}）",
            if more { "不少".to_string() } else { format!(" {left} 条") },
            offset + shown.len()
        );
    } else if mode == Mode::Content {
        let _ = write!(text, "\n（{files_hit} 个文件，{total_matches} 处）");
    }
    Ok(ToolResult::ok(text))
}

#[cfg(test)]
mod tests {
    use super::super::testing::TempProject;
    use super::*;
    use crate::gateway::event::PermissionLevel;
    use serde_json::json;

    fn project() -> TempProject {
        let p = TempProject::new();
        p.write(".gitignore", "dist/\n")
            .write("src/App.tsx", "import x\n<h1>Todo</h1>\nconst a = 1\n")
            .write("src/b.ts", "// <h1> in a comment\nlet y = 2\n")
            .write("dist/App.js", "<h1>Todo</h1>\n")
            .write("img.bin", "\0\0<h1>");
        p
    }

    #[test]
    fn literal_by_default_and_respects_gitignore() {
        let p = project();
        let ctx = p.ctx(PermissionLevel::Edit);
        let r = search_text(&ctx, &json!({"pattern":"<h1>"})).unwrap().text;
        assert!(r.contains("src/App.tsx:2: <h1>Todo</h1>"), "{r}");
        assert!(r.contains("src/b.ts:1:"));
        assert!(!r.contains("dist/") && !r.contains("img.bin"));
        assert!(r.contains("2 个文件，2 处"));
        let r = search_text(&ctx, &json!({"pattern":"a = .","regex":true})).unwrap().text;
        assert!(r.contains("src/App.tsx:3: const a = 1"));
        assert!(search_text(&ctx, &json!({"pattern":"a = ."})).unwrap().text.contains("没搜到"));
    }

    #[test]
    fn modes_glob_context_and_paging() {
        let p = project();
        let ctx = p.ctx(PermissionLevel::Edit);
        assert_eq!(search_text(&ctx, &json!({"pattern":"<h1>","mode":"files"})).unwrap().text, "src/App.tsx\nsrc/b.ts");
        assert_eq!(search_text(&ctx, &json!({"pattern":"<h1>","mode":"count","glob":"*.tsx"})).unwrap().text, "src/App.tsx:1");
        let r = search_text(&ctx, &json!({"pattern":"Todo","context":1,"path":"src"})).unwrap().text;
        assert!(r.contains("src/App.tsx-1- import x") && r.contains("src/App.tsx-3- const a = 1"), "{r}");
        let r = search_text(&ctx, &json!({"pattern":"<h1>","max":1})).unwrap().text;
        assert!(r.starts_with("src/App.tsx:2:") && r.contains("offset 设 1"), "{r}");
        let r = search_text(&ctx, &json!({"pattern":"<h1>","max":1,"offset":1})).unwrap().text;
        assert!(r.starts_with("src/b.ts:1:"), "{r}");
        assert!(search_text(&ctx, &json!({"pattern":"(","regex":true})).unwrap_err().contains("正则"));
    }
}
