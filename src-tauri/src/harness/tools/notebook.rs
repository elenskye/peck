//! `read_notebook`（v1.1.4）：读 Jupyter Notebook（`.ipynb`）——每个单元的类型、代码或文字，加上输出（文字、出错、
//! 图）。图（PNG、JPEG）给能看图的模型附上，一次最多 6 张。要改 Notebook 照旧用 `read_file` 读原文、用改文件的工具改 JSON。

use super::{str_arg, ToolContext};
use crate::gateway::ImageInput;
use serde_json::Value;

/// 一次最多附几张图。
const MAX_IMAGES: usize = 6;
/// 一个输出最多给多长。
const OUTPUT_BYTES: usize = 4 * 1024;
/// 一次给模型的最多多长（超了停在那个单元，告诉它从哪接着读）。
const TEXT_BYTES: usize = 100 * 1024;

/// `source`、`text` 这类字段：字符串，或者一行一个的数组。
fn joined(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// 去掉终端颜色（出错的 traceback 里全是）。
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clip(text: &str) -> String {
    let t = text.trim_end();
    if t.len() <= OUTPUT_BYTES {
        return t.to_string();
    }
    format!("{}\n…（输出太长，截断了，共 {} KB）", super::clip_bytes(t, OUTPUT_BYTES), t.len() / 1024)
}

/// `cells` → 单元号（从 1 起）的范围：`3`、`1-10`、`5-`。空的是全部。
fn cell_range(spec: Option<&str>, total: usize) -> Result<(usize, usize), String> {
    let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else { return Ok((1, total)) };
    let (a, b) = spec.split_once('-').unwrap_or((spec, spec));
    let a: usize = a.trim().parse().map_err(|_| format!("单元号看不懂：{spec}（写 3、1-10、5-）"))?;
    let b: usize = if b.trim().is_empty() { total } else { b.trim().parse().map_err(|_| format!("单元号看不懂：{spec}（写 3、1-10、5-）"))? };
    if a == 0 || a > b || a > total {
        return Err(format!("这个 Notebook 有 {total} 个单元，{spec} 不对。"));
    }
    Ok((a, b.min(total)))
}

/// 把一个 Notebook 拼成给模型看的样子。`vision` 为假时图只写一句。
pub fn render(rel: &str, nb: &Value, cells: Option<&str>, vision: bool) -> Result<(String, Vec<ImageInput>), String> {
    let list = nb["cells"].as_array().ok_or_else(|| format!("{rel} 不像 Jupyter Notebook（没有 cells）。"))?;
    let total = list.len();
    if total == 0 {
        return Ok((format!("{rel}：一个单元都没有。"), vec![]));
    }
    let (from, to) = cell_range(cells, total)?;
    let kernel = nb["metadata"]["kernelspec"]["display_name"].as_str().or_else(|| nb["metadata"]["language_info"]["name"].as_str()).unwrap_or("");
    let mut images = Vec::new();
    let mut body = String::new();
    let mut last = from - 1;
    let mut cut = false;
    for (i, cell) in list.iter().enumerate().take(to).skip(from - 1) {
        let kind = cell["cell_type"].as_str().unwrap_or("code");
        let mut block = match (kind, cell["execution_count"].as_u64()) {
            ("code", Some(n)) => format!("\n--- 单元 {} · code · [{n}] ---\n", i + 1),
            (kind, _) => format!("\n--- 单元 {} · {kind} ---\n", i + 1),
        };
        block.push_str(joined(&cell["source"]).trim_end());
        block.push('\n');
        let outputs = cell["outputs"].as_array().cloned().unwrap_or_default();
        if !outputs.is_empty() {
            block.push_str("输出：\n");
        }
        for out in &outputs {
            match out["output_type"].as_str().unwrap_or("") {
                "stream" => {
                    let name = out["name"].as_str().unwrap_or("stdout");
                    let text = clip(&strip_ansi(&joined(&out["text"])));
                    block.push_str(&if name == "stderr" { format!("[stderr]\n{text}\n") } else { format!("{text}\n") });
                }
                "error" => {
                    let trace: Vec<String> = out["traceback"].as_array().into_iter().flatten().filter_map(Value::as_str).map(strip_ansi).collect();
                    let head = format!("[出错] {}: {}", out["ename"].as_str().unwrap_or("Error"), out["evalue"].as_str().unwrap_or(""));
                    block.push_str(&format!("{}\n", clip(&if trace.is_empty() { head } else { format!("{head}\n{}", trace.join("\n")) })));
                }
                "execute_result" | "display_data" => {
                    let data = &out["data"];
                    let picture = ["image/png", "image/jpeg"].into_iter().find_map(|t| data[t].as_str().map(|d| (t, d)));
                    match picture {
                        Some((media_type, raw)) if vision && images.len() < MAX_IMAGES => {
                            images.push(ImageInput { media_type: media_type.into(), data: raw.chars().filter(|c| !c.is_whitespace()).collect() });
                            block.push_str(&format!("[图 {}（{media_type}），附在结果后面]\n", images.len()));
                        }
                        Some((media_type, _)) => {
                            let why = if vision { "这次附的图满了，单独读这个单元能拿到" } else { "这个模型看不了图" };
                            block.push_str(&format!("[一张图（{media_type}），{why}]\n"));
                        }
                        None => {}
                    }
                    let text = ["text/markdown", "text/plain"].into_iter().find_map(|t| data.get(t).map(joined)).filter(|t| !t.trim().is_empty());
                    if let Some(text) = text {
                        block.push_str(&format!("{}\n", clip(&strip_ansi(&text))));
                    } else if picture.is_none() && data.get("text/html").is_some() {
                        block.push_str("[HTML 输出，没有纯文字版]\n");
                    }
                }
                _ => {}
            }
        }
        if body.len() + block.len() > TEXT_BYTES && !body.is_empty() {
            cut = true;
            break;
        }
        body.push_str(&block);
        last = i + 1;
    }
    let range = if from == last { format!("单元 {from}") } else { format!("单元 {from}–{last}") };
    let mut head = format!("{rel}：{}共 {total} 个单元，这次是{range}。", if kernel.is_empty() { String::new() } else { format!("{kernel} · ") });
    if !images.is_empty() {
        head.push_str(&format!("附了 {} 张输出里的图，在结果后面。", images.len()));
    }
    let mut text = format!("{head}\n{body}");
    if cut {
        text.push_str(&format!("\n（超过 100KB，停在单元 {last}；下次从单元 {} 读，`cells` 写 {}-）", last + 1, last + 1));
    }
    Ok((text, images))
}

/// `read_notebook`。
pub fn read_notebook(ctx: &ToolContext, input: &Value, vision: bool) -> Result<(String, Vec<ImageInput>), String> {
    let raw = str_arg(input, "path").ok_or("缺 path。")?;
    let file = ctx.resolve_path(raw, false)?;
    let rel = ctx.rel(&file);
    let bytes = std::fs::read(&file).map_err(|_| format!("读不到 {rel}（不存在或没权限）。"))?;
    let nb: Value = serde_json::from_slice(&bytes).map_err(|_| format!("{rel} 不是 JSON，不像 Jupyter Notebook。"))?;
    render(&rel, &nb, str_arg(input, "cells"), vision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "metadata": {"kernelspec": {"display_name": "Python 3"}},
            "cells": [
                {"cell_type": "markdown", "source": ["# 标题\n", "说明"]},
                {"cell_type": "code", "execution_count": 2, "source": "print('hi')", "outputs": [
                    {"output_type": "stream", "name": "stdout", "text": ["hi\n"]},
                    {"output_type": "display_data", "data": {"image/png": "iVBORw0KGgo=\n", "text/plain": ["<Figure>"]}}
                ]},
                {"cell_type": "code", "execution_count": 3, "source": "1/0", "outputs": [
                    {"output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero", "traceback": ["\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero"]}
                ]}
            ]
        })
    }

    #[test]
    fn cells_outputs_errors_and_images() {
        let (text, images) = render("a.ipynb", &sample(), None, true).unwrap();
        assert!(text.starts_with("a.ipynb：Python 3 · 共 3 个单元，这次是单元 1–3。附了 1 张"), "{text}");
        assert!(text.contains("--- 单元 1 · markdown ---\n# 标题\n说明"));
        assert!(text.contains("--- 单元 2 · code · [2] ---\nprint('hi')\n输出：\nhi\n[图 1（image/png），附在结果后面]\n<Figure>"));
        assert!(text.contains("[出错] ZeroDivisionError: division by zero\nZeroDivisionError: division by zero"), "颜色去掉了：{text}");
        assert_eq!(images, vec![ImageInput { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() }]);
        let (text, images) = render("a.ipynb", &sample(), Some("2"), false).unwrap();
        assert!(text.contains("这次是单元 2。") && text.contains("这个模型看不了图") && images.is_empty());
        assert!(render("a.ipynb", &sample(), Some("5"), true).is_err());
        assert!(render("a.json", &json!({"x": 1}), None, true).unwrap_err().contains("没有 cells"));
    }
}
