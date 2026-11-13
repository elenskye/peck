//! 自动压缩（v0.6 Phase 7）：上下文估到压缩线（模型窗口的 80%）时，下一次请求前按顺序做，
//! 降到目标线（60%）以下就停：
//!
//! 1. **落盘**：旧的、长的工具输出整份写进这个任务的输出目录，上下文里只留开头、结尾和路径（`read_file` 能读回来）；
//! 2. **清旧工具输出**：除了最近两轮，工具输出换成一句「做过什么」；旧的思考、旧的环境块也去掉；
//! 3. **交接摘要**：让模型写一份交接摘要（目标、做了什么、现在的状态、下一步），前面的对话换成它 + 站主最近说的几句。
//!
//! 第 3 步要问模型，在循环里做（`loop.rs`）；这里是不用问模型的部分。

use super::context::{CONTEXT_TAG, REF_TAG};
use super::tools::{self, clip_bytes, tail_bytes};
use crate::gateway::{Message, Part, Role};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// 到窗口的这个比例开始压缩（界面上的「压缩线」）。
pub const COMPACT_AT: f64 = 0.80;
/// 压到这个比例以下就不再往下一步做。
pub const TARGET: f64 = 0.60;
/// 压完还在这个比例以上就停下（不发请求）。
pub const STOP_AT: f64 = 0.95;
/// 第 1 步：比这个长的旧工具输出落盘。
pub const SPILL_OVER: usize = 3 * 1024;
/// 第 2 步：最近几轮的工具输出不动。
pub const KEEP_ROUNDS: usize = 2;
/// 认出验证结果（自动修交回去的那条）。
pub const VERIFY_TAG: &str = "<peck-verify>";
/// 认出交接摘要。
pub const HANDOFF_TAG: &str = "<peck-handoff>";

/// 第 1 步：旧的长工具输出落盘。最后一条消息（刚跑完的工具结果）不动。返回落了几份。
pub fn spill_large(messages: &mut [Message], spill_dir: &Path, counter: &AtomicU32) -> usize {
    let last = messages.len().saturating_sub(1);
    let mut spilled = 0;
    for m in messages.iter_mut().take(last) {
        for p in &mut m.parts {
            let Part::ToolResult { output, .. } = p else { continue };
            if output.len() <= SPILL_OVER || output.contains("[压缩上下文时落盘") {
                continue;
            }
            if std::fs::create_dir_all(spill_dir).is_err() {
                return spilled;
            }
            // 输出目录按任务（接着发的几次共用）：已经有的不覆盖，前几次的对话还指着它们。
            let mut path;
            loop {
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                path = spill_dir.join(format!("ctx-{n}.log"));
                if !path.exists() {
                    break;
                }
            }
            if std::fs::write(&path, output.as_bytes()).is_err() {
                continue;
            }
            let kb = output.len().div_ceil(1024);
            *output = format!(
                "{}\n…\n{}\n[压缩上下文时落盘：这份输出共 {kb} KB，完整的存在 {}（用 read_file 分段读）]",
                clip_bytes(output, 800),
                tail_bytes(output, 800),
                path.display()
            );
            spilled += 1;
        }
    }
    spilled
}

/// 第 2 步：最近 [`KEEP_ROUNDS`] 轮以外的工具输出换成一句话；旧的思考、旧的环境块去掉。返回清了几份输出。
pub fn clear_old(messages: &mut [Message]) -> usize {
    // 工具调用的 id → 给人看的一行（「读 src/App.tsx」），清掉的输出写它。
    let mut titles: HashMap<String, String> = HashMap::new();
    for m in messages.iter() {
        for p in &m.parts {
            if let Part::ToolCall { call_id, name, input, .. } = p {
                titles.insert(call_id.clone(), tools::title_of(name, input));
            }
        }
    }
    let with_results: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::User && m.parts.iter().any(|p| matches!(p, Part::ToolResult { .. })))
        .map(|(i, _)| i)
        .collect();
    let keep_from = with_results.iter().rev().nth(KEEP_ROUNDS - 1).copied().unwrap_or(0);
    let last_assistant = messages.iter().rposition(|m| m.role == Role::Assistant);
    // 这一回合（站主最近一句以后）的思考都留着：DeepSeek 这类要在工具回合里把思考原样传回去。
    let turn_start = messages
        .iter()
        .rposition(|m| m.role == Role::User && !m.parts.iter().any(|p| matches!(p, Part::ToolResult { .. })))
        .unwrap_or(0);
    let last_context = messages
        .iter()
        .rposition(|m| m.parts.iter().any(|p| matches!(p, Part::Text { text } if text.starts_with(CONTEXT_TAG))));
    let mut cleared = 0;
    for (i, m) in messages.iter_mut().enumerate() {
        if m.role == Role::Assistant && i < turn_start && Some(i) != last_assistant {
            m.parts.retain(|p| !matches!(p, Part::Reasoning { .. }));
        }
        if Some(i) != last_context {
            m.parts.retain(|p| !matches!(p, Part::Text { text } if text.starts_with(CONTEXT_TAG)));
        }
        if i < keep_from {
            for p in &mut m.parts {
                if let Part::ToolResult { call_id, output, .. } = p {
                    if output.len() > 160 && !output.starts_with("[旧的工具输出已清掉") {
                        let what = titles.get(call_id).cloned().unwrap_or_else(|| "一步工具调用".into());
                        *output = format!("[旧的工具输出已清掉（压缩上下文）：{what}。要用再读一次或再跑一次。]");
                        cleared += 1;
                    }
                }
            }
        }
        if m.parts.is_empty() {
            m.parts.push(Part::Text { text: "（压缩上下文时这一条清空了。）".into() });
        }
    }
    cleared
}

/// 站主自己写的几句（不算环境块、引用、验证结果、交接摘要、工具输出），最近的 `n` 句，旧的在前。
pub fn user_requests(messages: &[Message], n: usize) -> Vec<String> {
    let mut out: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::User)
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::Text { text }
                if !text.starts_with(CONTEXT_TAG)
                    && !text.starts_with(REF_TAG)
                    && !text.starts_with(VERIFY_TAG)
                    && !text.starts_with(HANDOFF_TAG) =>
            {
                Some(clip_bytes(text.trim(), 1500).to_string())
            }
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect();
    let skip = out.len().saturating_sub(n);
    out.drain(..skip);
    out
}

/// 第 3 步：问模型要交接摘要时，接在对话最后一条（站主那一条）后面的话。
pub const SUMMARY_REQUEST: &str = "<peck-compact>\nThe context window is getting full. Before continuing, write a handoff summary so you can pick up the work in a fresh context. Do not call any tools now.\n\nInclude, in Markdown, at most about 800 words:\n1. The user's goal and every explicit request, constraint or decision (quote short user messages verbatim).\n2. What has been done: files read and changed (paths) and key findings.\n3. Current state: what works, what fails (exact error lines), commands run and their results.\n4. What remains and the very next step.\n</peck-compact>";

/// 第 3 步：压缩以后的对话：一条站主消息，带交接摘要和站主最近说的几句。
pub fn rebuild(summary: &str, requests: &[String]) -> Vec<Message> {
    let mut text = format!("{HANDOFF_TAG}\n{}\n</peck-handoff>\n\nThe earlier conversation was compacted into the handoff summary above (written by you). ", summary.trim());
    if !requests.is_empty() {
        text.push_str("The user's most recent messages, verbatim:\n");
        for r in requests {
            text.push_str(&format!("> {}\n", r.replace('\n', "\n> ")));
        }
        text.push('\n');
    }
    text.push_str("Continue the task from where it stopped. Re-read files before editing them if you need their exact text.");
    vec![Message { role: Role::User, parts: vec![Part::Text { text }] }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(id: &str, path: &str) -> Message {
        Message { role: Role::Assistant, parts: vec![
            Part::Reasoning { text: "想一下".into(), opaque: None },
            Part::ToolCall { call_id: id.into(), name: "read_file".into(), input: json!({"path": path}), signature: None },
        ] }
    }
    fn result(id: &str, output: String) -> Message {
        Message { role: Role::User, parts: vec![Part::ToolResult { call_id: id.into(), output, is_error: false }] }
    }

    fn sample() -> Vec<Message> {
        let mut m = vec![Message { role: Role::User, parts: vec![
            Part::Text { text: format!("{CONTEXT_TAG}\nold\n</peck-context>") },
            Part::Text { text: "把标题改成中文".into() },
        ] }];
        for i in 0..4 {
            m.push(call(&format!("c{i}"), &format!("f{i}.rs")));
            m.push(result(&format!("c{i}"), format!("{}{}", "x".repeat(5000), i)));
        }
        m
    }

    #[test]
    fn spilling_keeps_head_tail_and_path_but_not_the_latest() {
        let dir = std::env::temp_dir().join(format!("peck-compact-{}", uuid::Uuid::new_v4()));
        let mut m = sample();
        let n = spill_large(&mut m, &dir, &AtomicU32::new(0));
        assert_eq!(n, 3, "最后一条刚跑完的不动");
        let Part::ToolResult { output, .. } = &m[2].parts[0] else { panic!() };
        assert!(output.len() < 2000 && output.contains("完整的存在") && output.contains("ctx-1.log"));
        assert_eq!(std::fs::read_to_string(dir.join("ctx-1.log")).unwrap().len(), 5001);
        let Part::ToolResult { output, .. } = &m[8].parts[0] else { panic!() };
        assert_eq!(output.len(), 5001);
        assert_eq!(spill_large(&mut m, &dir, &AtomicU32::new(10)), 0, "落过的不再落");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clearing_keeps_the_last_two_rounds_and_says_what_was_done() {
        let mut m = sample();
        m.push(Message { role: Role::Assistant, parts: vec![Part::Text { text: "好".into() }] });
        m.push(Message { role: Role::User, parts: vec![
            Part::Text { text: format!("{CONTEXT_TAG}\nnew\n</peck-context>") },
            Part::Text { text: "再改副标题".into() },
        ] });
        let n = clear_old(&mut m);
        assert_eq!(n, 2);
        let Part::ToolResult { output, .. } = &m[2].parts[0] else { panic!() };
        assert!(output.contains("读 f0.rs"), "{output}");
        let Part::ToolResult { output, .. } = &m[6].parts[0] else { panic!() };
        assert!(output.starts_with("xxx"), "最近两轮不动");
        // 旧的环境块去掉、最新的留着；旧的思考去掉
        assert_eq!(m[0].parts.len(), 1);
        assert!(matches!(&m[10].parts[0], Part::Text { text } if text.contains("new")));
        assert!(!m[1].parts.iter().any(|p| matches!(p, Part::Reasoning { .. })));
        // 工具调用都还在，对话接得上
        assert_eq!(m.iter().flat_map(|x| &x.parts).filter(|p| matches!(p, Part::ToolCall { .. })).count(), 4);
        assert_eq!(clear_old(&mut m), 0);
    }

    #[test]
    fn rebuilding_keeps_what_the_user_said() {
        let mut m = sample();
        m.push(Message { role: Role::User, parts: vec![Part::Text { text: format!("{VERIFY_TAG}\nfail") }] });
        let asks = user_requests(&m, 3);
        assert_eq!(asks, vec!["把标题改成中文".to_string()]);
        let new = rebuild("## 目标\n改标题", &asks);
        assert_eq!(new.len(), 1);
        let Part::Text { text } = &new[0].parts[0] else { panic!() };
        assert!(text.starts_with(HANDOFF_TAG) && text.contains("> 把标题改成中文") && text.contains("Continue"));
        assert!(user_requests(&new, 3).is_empty(), "交接摘要不算站主说的");
    }
}
