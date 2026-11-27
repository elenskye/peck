//! 压缩上下文（v0.6 Phase 7；v1.1.7 加了手动压缩和压完放回）。三步：
//!
//! 1. **落盘**：旧的、长的工具输出整份写进这个任务的输出目录，上下文里只留开头、结尾和路径（`read_file` 能读回来）；
//! 2. **清旧工具输出**：除了最近两轮，工具输出换成一句「做过什么」（`tool_search` 留着「已加载：…」那一行）；旧的思考、旧的环境块也去掉；
//! 3. **交接摘要**：让模型照固定五栏写一份交接摘要（长度跟着窗口），前面的对话换成它 + 站主说过的话（ASK 的回答也算，
//!    再压一次也原样带着），再放回压缩会丢掉的东西：新的环境块、加载过的 MCP 工具、最近读过和改过的文件（现读）。
//!
//! 自动压缩：上下文估到压缩线（窗口的 80%）时按顺序做，降到目标线（60%）以下就停。手动 `/compact [重点]`：三步都做。
//! 第 3 步要问模型，在循环里做（`loop.rs`）；这里是不用问模型的部分。

use super::context::{self, CONTEXT_TAG, REF_TAG};
use super::tools::{self, clip_bytes, tail_bytes, ToolContext};
use crate::gateway::{Message, Part, Role};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
/// 认出交接摘要。
pub const HANDOFF_TAG: &str = "<peck-handoff>";
/// 压缩后放回的最近的文件。
pub const RESTORED_TAG: &str = "<peck-restored-files>";
/// 压缩后记下的加载过的 MCP 工具（下一句话开头从这里认回来，工具列表不变）。
pub const LOADED_TAG: &str = "<peck-loaded-tools>";
/// 压缩后接着干的那一句。
pub const RESUME_TAG: &str = "<peck-resume>";
/// 压缩以后原样带着的站主的话（v1.3.0 单独一块：再压一次时从这里认回来）。
pub const SAID_TAG: &str = "<peck-said>";
/// `ask_user` 的结果开头：站主在 ASK 卡片里答的（算站主说的话，压缩不清、不落盘）。
pub const ANSWERED: &str = "站主回答：";
const SKIPPED_HEAD: &str = "（中间还有 ";
const SKIPPED_TAIL: &str = " 句，见摘要）";
/// `tool_search` 结果的头一行。
pub const LOADED: &str = "已加载：";
/// 放回最多几个文件、一共占窗口的多少。
pub const RESTORE_FILES: usize = 5;
pub const RESTORE_SHARE: f64 = 0.10;
/// 放回的文件比这个大就跳过。
const RESTORE_MAX_BYTES: u64 = 1024 * 1024;
/// 不知道窗口多大时按这么大算。
const UNKNOWN_WINDOW: u64 = 128_000;
/// 清掉的旧输出开头写这一句。
const CLEARED: &str = "[旧的工具输出已清掉";

/// 第 1 步：旧的长工具输出落盘。最后一条消息（刚跑完的工具结果）、站主在 ASK 里的回答不动。返回落了几份。
pub fn spill_large(messages: &mut [Message], spill_dir: &Path, counter: &AtomicU32) -> usize {
    let last = messages.len().saturating_sub(1);
    let mut spilled = 0;
    for m in messages.iter_mut().take(last) {
        for p in &mut m.parts {
            let Part::ToolResult { output, .. } = p else { continue };
            if output.len() <= SPILL_OVER || output.contains("[压缩上下文时落盘") || output.contains(CLEARED) || output.starts_with(ANSWERED) {
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

/// 第 2 步：最近 [`KEEP_ROUNDS`] 轮以外的工具输出换成一句话（站主在 ASK 里的回答留着）；旧的思考、旧的环境块去掉。返回清了几份输出。
/// `tool_search` 的输出留着头一行「已加载：…」（下一句话开头靠它认回加载过的工具）。
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
                    if output.len() > 160 && !output.contains(CLEARED) && !output.starts_with(ANSWERED) {
                        let what = titles.get(call_id).cloned().unwrap_or_else(|| "一步工具调用".into());
                        let note = format!("{CLEARED}（压缩上下文）：{what}。要用再读一次或再跑一次。]");
                        *output = match output.lines().next().filter(|l| l.starts_with(LOADED)) {
                            Some(line) => format!("{line}\n{note}"),
                            None => note,
                        };
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

// ───────────────────────── 第 3 步：交接摘要 ─────────────────────────

/// 交接摘要的目标长度（tokens）：窗口的 1.5%，1500 到 8000 之间。
pub fn summary_target(window: Option<u64>) -> u64 {
    ((window.unwrap_or(UNKNOWN_WINDOW) as f64 * 0.015) as u64).clamp(1500, 8000)
}

/// 写摘要那一次最多输出多少：思考也算在输出里，给够（模型自己的上限以内）。
pub fn summary_max_output(window: Option<u64>, model_max: u64) -> u32 {
    model_max.min((summary_target(window) * 2).max(16_000)).min(u32::MAX as u64) as u32
}

/// 问模型要交接摘要时，接在对话最后一条（站主那一条）后面的话。`focus` 是站主 `/compact` 后面写的重点。
/// 固定五栏（v1.3.0）：长任务压几次以后，定下的事、踩过的坑不靠模型自己想起来写。
pub fn summary_request(window: Option<u64>, focus: Option<&str>) -> String {
    let words = ((summary_target(window) as f64 * 0.75 / 100.0).round() as u64 * 100).max(100);
    let focus = focus
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| format!("\n\nThe user asked you to keep this in particular, in detail: {f}"))
        .unwrap_or_default();
    format!(
        "<peck-compact>\nThe conversation is being compacted. Write a handoff summary so you can pick up the work in a fresh context. Do not call any tools now.\n\nUse exactly these five Markdown headings, in this order, at most about {words} words in all. Keep the headings as written; write the items in the user's language, short and concrete; put （无） under a heading with nothing in it. What the user said is carried over verbatim separately: don't copy it, just keep what it settled.\n\n## 目标\nThe user's goal and every explicit request or constraint.\n\n## 已定的决策\nEvery decision made so far, by the user (answers to your questions included) or by you: what was chosen and why. These are settled: don't reopen them unless the user does.\n\n## 踩过的坑\nWhat failed or turned out wrong: approaches tried, exact error lines, wrong assumptions, and what works instead. Don't repeat them.\n\n## 文件现在的状态\nEach file read or changed, one line each: path — done / partly done (what is missing) / only read (what matters in it).\n\n## 下一步\nWhat remains, then the very next step.\n\nIf an earlier handoff summary (<peck-handoff>) is in the conversation, carry every decision and pitfall in it over unless it was overturned since.{focus}\n</peck-compact>"
    )
}

/// 站主说过的话（重建时原样带上）：`first` 是第一句（原来的目标），`skipped` 句放不下省掉了，`rest` 是接着的（旧的在前）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Requests {
    pub first: Option<String>,
    pub skipped: usize,
    pub rest: Vec<String>,
}

impl Requests {
    pub fn is_empty(&self) -> bool {
        self.first.is_none() && self.rest.is_empty()
    }

    /// 全部带上的（没省的）几句，旧的在前。
    #[cfg(test)]
    pub fn all(&self) -> Vec<String> {
        self.first.iter().cloned().chain(self.rest.iter().cloned()).collect()
    }
}

/// 是不是 Peck 自己加的话（环境块、引用、交接摘要、放回的文件、Hook 补充的话……），不算站主说的。
fn added_by_peck(text: &str) -> bool {
    text.starts_with("<peck-") || text.starts_with(REF_TAG) || text.starts_with("<hook-")
}

/// `ask_user` 调用里问的几个问题。
fn asked(input: &serde_json::Value) -> Vec<String> {
    let q = |v: &serde_json::Value| v.get("question").and_then(serde_json::Value::as_str).map(|t| t.trim().to_string());
    match input.get("questions").and_then(serde_json::Value::as_array) {
        Some(qs) if !qs.is_empty() => qs.iter().filter_map(q).collect(),
        _ => q(input).into_iter().collect(),
    }
}

/// 站主在 ASK 里的回答写成一句站主的话：一个问题的带上问题（结果里没有），几个的结果里一问一行。
fn answer_line(output: &str, questions: &[String]) -> String {
    let answer = output.strip_prefix(ANSWERED).unwrap_or(output).trim();
    match questions {
        [q] if !answer.contains('\n') => format!("（回答「{q}」）{answer}"),
        _ => format!("（回答提问）\n{answer}"),
    }
}

/// 上一次压缩带着的站主的话（`<peck-said>` 那一块）：几句、当时省掉了几句。
fn said_before(text: &str) -> (Vec<String>, usize) {
    let body = text.strip_prefix(SAID_TAG).unwrap_or(text);
    let body = body.split("</peck-said>").next().unwrap_or(body);
    let (mut said, mut skipped) = (Vec::new(), 0);
    for chunk in body.split("\n\n").map(|c| c.trim_matches('\n')) {
        if chunk.starts_with('>') {
            said.push(chunk.lines().map(|l| l.strip_prefix("> ").or_else(|| l.strip_prefix('>')).unwrap_or(l)).collect::<Vec<_>>().join("\n"));
        } else if let Some(n) = chunk.strip_prefix(SKIPPED_HEAD).and_then(|r| r.strip_suffix(SKIPPED_TAIL)).and_then(|n| n.parse::<usize>().ok()) {
            skipped += n;
        }
    }
    (said, skipped)
}

/// 站主自己说的每一句（不算环境块、引用、交接摘要、Hook 补充的话、别的工具输出），按先后：写的话、ASK 里的回答、
/// 上一次压缩带着的（`<peck-said>`，所以压几次都还在）。每句最多 1500 字节；一共不超过窗口的 3%（至少 2000 tokens），
/// 超了留第一句（原来的目标）和最近的，中间的省掉。
pub fn user_requests(messages: &[Message], window: Option<u64>) -> Requests {
    let questions: HashMap<&str, Vec<String>> = messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::ToolCall { call_id, name, input, .. } if name == tools::ASK_USER => Some((call_id.as_str(), asked(input))),
            _ => None,
        })
        .collect();
    let (mut said, mut skipped) = (Vec::new(), 0);
    for p in messages.iter().filter(|m| m.role == Role::User).flat_map(|m| m.parts.iter()) {
        match p {
            Part::Text { text } if text.starts_with(SAID_TAG) => {
                let (before, n) = said_before(text);
                said.extend(before);
                skipped += n;
            }
            Part::Text { text } if !added_by_peck(text) => said.push(text.trim().to_string()),
            Part::ToolResult { call_id, output, .. } if output.starts_with(ANSWERED) => {
                said.push(answer_line(output, questions.get(call_id.as_str()).map(Vec::as_slice).unwrap_or_default()));
            }
            _ => {}
        }
    }
    let said: Vec<String> = said.iter().map(|t| clip_bytes(t, 1500).to_string()).filter(|t| !t.is_empty()).collect();
    let budget = ((window.unwrap_or(UNKNOWN_WINDOW) as f64 * 0.03) as u64).max(2000);
    let total: u64 = said.iter().map(|s| context::estimate(s)).sum();
    let mut it = said.into_iter();
    let Some(first) = it.next() else { return Requests::default() };
    let later: Vec<String> = it.collect();
    if total <= budget {
        return Requests { first: Some(first), skipped, rest: later };
    }
    let mut left = budget.saturating_sub(context::estimate(&first));
    let mut rest = Vec::new();
    for s in later.iter().rev() {
        let n = context::estimate(s);
        if n > left {
            break;
        }
        left -= n;
        rest.push(s.clone());
    }
    rest.reverse();
    Requests { first: Some(first), skipped: skipped + later.len() - rest.len(), rest }
}

/// 重建时放回去的东西（第 3 步以后）。
#[derive(Debug, Clone, Default)]
pub struct Restore {
    /// 新的环境块（`<peck-context>`）。
    pub env: Option<String>,
    /// 加载过的 MCP 工具。
    pub loaded: Vec<String>,
    /// 放回的文件（`<peck-restored-files>` 那一块）。
    pub files: Option<String>,
}

/// 第 3 步：压缩以后的对话：一条站主消息。顺序：交接摘要 → 站主说过的话（`<peck-said>`）→ 环境块 → 加载过的 MCP 工具 → 放回的文件 → 接着干。
pub fn rebuild(summary: &str, requests: &Requests, restore: &Restore) -> Vec<Message> {
    let text = format!("{HANDOFF_TAG}\n{}\n</peck-handoff>\n\nThe earlier conversation was compacted into the handoff summary above (written by you).", summary.trim());
    let mut parts = vec![Part::Text { text }];
    if !requests.is_empty() {
        // 一句一段（引用里的空行写成「> 」），再压一次时 `said_before` 照段认回来。
        let quote = |t: &str| format!("> {}", t.replace('\n', "\n> "));
        let mut chunks = vec!["Everything the user said, verbatim, oldest first (answers to your questions included):".to_string()];
        chunks.extend(requests.first.as_deref().map(quote));
        if requests.skipped > 0 {
            chunks.push(format!("{SKIPPED_HEAD}{}{SKIPPED_TAIL}", requests.skipped));
        }
        chunks.extend(requests.rest.iter().map(|r| quote(r)));
        parts.push(Part::Text { text: format!("{SAID_TAG}\n{}\n</peck-said>", chunks.join("\n\n")) });
    }
    if let Some(env) = &restore.env {
        parts.push(Part::Text { text: env.clone() });
    }
    if !restore.loaded.is_empty() {
        parts.push(Part::Text { text: format!("{LOADED_TAG}{}</peck-loaded-tools>\nThese MCP tools were loaded before the compaction and are still callable.", restore.loaded.join(", ")) });
    }
    if let Some(files) = &restore.files {
        parts.push(Part::Text { text: files.clone() });
    }
    let resume = if restore.files.is_some() {
        "Continue the task from where it stopped. The most recent files were restored above, fresh from disk: you don't need to re-read them before editing."
    } else {
        "Continue the task from where it stopped. Re-read files before editing them if you need their exact text."
    };
    parts.push(Part::Text { text: format!("{RESUME_TAG}\n{resume}\n</peck-resume>") });
    vec![Message { role: Role::User, parts }]
}

/// 对话里加载过、调过的 MCP 工具（调过的、`tool_search` 结果头一行、压缩时记下的），按先后、不去重以外不筛。
pub fn loaded_in(messages: &[Message]) -> Vec<String> {
    let searches: std::collections::HashSet<&str> = messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::ToolCall { call_id, name, .. } if name == tools::TOOL_SEARCH => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    for p in messages.iter().flat_map(|m| m.parts.iter()) {
        let names: Vec<String> = match p {
            Part::ToolCall { name, .. } if name.starts_with("mcp__") => vec![name.clone()],
            Part::ToolResult { call_id, output, .. } if searches.contains(call_id.as_str()) => loaded_line(output),
            Part::Text { text } if text.starts_with(LOADED_TAG) => text[LOADED_TAG.len()..]
                .split("</peck-loaded-tools>")
                .next()
                .unwrap_or("")
                .split(',')
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect(),
            _ => vec![],
        };
        for n in names {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// `tool_search` 结果头一行里的名字。
pub fn loaded_line(output: &str) -> Vec<String> {
    output.lines().next().and_then(|l| l.strip_prefix(LOADED)).map(|l| l.split('、').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()).unwrap_or_default()
}

// ───────────────────────── 放回最近的文件 ─────────────────────────

/// 对话里文件工具碰过的路径（模型给的原样），最后一次碰的在前；改过的（这次对话里改过、或者在 `changed` 里）排前面。
pub fn recent_files(messages: &[Message], changed: &[String]) -> Vec<String> {
    let mut seen: Vec<(String, usize, bool)> = Vec::new();
    let mut n = 0usize;
    for p in messages.iter().flat_map(|m| m.parts.iter()) {
        let Part::ToolCall { name, input, .. } = p else { continue };
        let (paths, edit): (Vec<String>, bool) = match name.as_str() {
            tools::READ_FILE => (tools::str_arg(input, "path").map(|s| vec![s.to_string()]).unwrap_or_default(), false),
            tools::WRITE_FILE | tools::WRITE_PATCH | tools::APPLY_PATCH => (tools::write_targets(name, input), true),
            _ => continue,
        };
        for raw in paths {
            n += 1;
            let raw = raw.trim().to_string();
            if raw.is_empty() {
                continue;
            }
            match seen.iter_mut().find(|(p, _, _)| *p == raw) {
                Some(e) => {
                    e.1 = n;
                    e.2 |= edit;
                }
                None => seen.push((raw, n, edit)),
            }
        }
    }
    let norm = |p: &str| p.trim_start_matches("./").to_string();
    for e in &mut seen {
        if changed.iter().any(|c| norm(c) == norm(&e.0) || e.0.ends_with(&format!("/{}", norm(c)))) {
            e.2 = true;
        }
    }
    seen.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
    seen.into_iter().map(|(p, _, _)| p).collect()
}

/// 一个要放回的文件：读到的样子和带行号的每一行。
struct Fresh {
    path: PathBuf,
    rel: String,
    bytes: Vec<u8>,
    rows: Vec<String>,
    tokens: u64,
}

/// 把几个文件从磁盘现读出来、放进 `<peck-restored-files>`：最多 [`RESTORE_FILES`] 个，一共不超过窗口的 [`RESTORE_SHARE`]；
/// 放不下的留头尾、中间写省略；删了的、文件夹、二进制、太大的跳过。放回的登记成「刚读过」。
pub fn restore_files(ctx: &ToolContext, candidates: &[String], window: Option<u64>) -> Option<String> {
    let budget = (window.unwrap_or(UNKNOWN_WINDOW) as f64 * RESTORE_SHARE) as u64;
    let mut picked: Vec<Fresh> = Vec::new();
    for raw in candidates {
        if picked.len() >= RESTORE_FILES {
            break;
        }
        let Ok(path) = ctx.resolve_path(raw, false) else { continue };
        if picked.iter().any(|f| f.path == path) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() || meta.len() > RESTORE_MAX_BYTES || meta.len() == 0 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let rows: Vec<String> = text
            .lines()
            .enumerate()
            .map(|(i, line)| {
                format!("{:>6}\t{}", i + 1, tools::fs::shown_line(line, false))
            })
            .collect();
        let tokens = rows.iter().map(|r| context::estimate(r) + 1).sum();
        picked.push(Fresh { rel: ctx.rel(&path), path, bytes, rows, tokens });
    }
    if picked.is_empty() {
        return None;
    }
    let caps = water_fill(&picked.iter().map(|f| f.tokens).collect::<Vec<_>>(), budget);
    let mut body = String::new();
    let mut shown = 0;
    for (f, cap) in picked.iter().zip(caps) {
        // 分到的太少（又放不下全文）就不放：半截的几行没用。
        if cap < f.tokens && cap < 400 {
            continue;
        }
        let total = f.rows.len();
        let (lines, cut) = if cap >= f.tokens { (f.rows.join("\n"), None) } else { head_tail(&f.rows, cap) };
        let head = match cut {
            Some((a, b)) => format!("=== {}（共 {total} 行，中间第 {a}–{b} 行省略了：完整的用 read_file 读）===", f.rel),
            None => format!("=== {}（共 {total} 行）===", f.rel),
        };
        body.push_str(&format!("{head}\n{lines}\n\n"));
        ctx.remember_read(&f.path, &f.bytes);
        shown += 1;
    }
    (shown > 0).then(|| {
        format!(
            "{RESTORED_TAG}\nThe files you read or changed most recently, read fresh from disk after the compaction (line-numbered like read_file):\n\n{}</peck-restored-files>",
            body
        )
    })
}

/// 按「小的先满足、剩下的平分」把预算分给几份（max-min 公平）。
fn water_fill(sizes: &[u64], budget: u64) -> Vec<u64> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| sizes[i]);
    let mut caps = vec![0; sizes.len()];
    let mut left = budget;
    for (k, &i) in order.iter().enumerate() {
        let share = left / (sizes.len() - k) as u64;
        caps[i] = sizes[i].min(share);
        left -= caps[i];
    }
    caps
}

/// 留开头和结尾几行，中间省掉：返回拼好的几行、省掉的是第几到第几行。
fn head_tail(rows: &[String], cap: u64) -> (String, Option<(usize, usize)>) {
    let cost = |r: &String| context::estimate(r) + 1;
    let (mut head, mut used) = (0usize, 0u64);
    while head < rows.len() && used + cost(&rows[head]) <= cap * 6 / 10 {
        used += cost(&rows[head]);
        head += 1;
    }
    let mut tail = rows.len();
    while tail > head && used + cost(&rows[tail - 1]) <= cap {
        tail -= 1;
        used += cost(&rows[tail]);
    }
    if tail <= head {
        return (rows.join("\n"), None);
    }
    let mut out: Vec<String> = rows[..head].to_vec();
    out.push(format!("      …（第 {}–{} 行省略）", head + 1, tail));
    out.extend(rows[tail..].iter().cloned());
    (out.join("\n"), Some((head + 1, tail)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn call(id: &str, path: &str) -> Message {
        Message { role: Role::Assistant, parts: vec![
            Part::Reasoning { text: "想一下".into(), opaque: None },
            Part::ToolCall { call_id: id.into(), name: "read_file".into(), input: json!({"path": path}), signature: None },
        ] }
    }
    fn result(id: &str, output: String) -> Message {
        Message { role: Role::User, parts: vec![Part::ToolResult { call_id: id.into(), output, is_error: false }] }
    }
    fn said(text: &str) -> Message {
        Message { role: Role::User, parts: vec![Part::Text { text: text.into() }] }
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
    fn clearing_a_tool_search_keeps_the_loaded_line() {
        let mut m = vec![said("用 GitHub 的工具查 issue")];
        m.push(Message { role: Role::Assistant, parts: vec![Part::ToolCall { call_id: "s".into(), name: tools::TOOL_SEARCH.into(), input: json!({"query": "select:mcp__gh__issues"}), signature: None }] });
        m.push(result("s", format!("{LOADED}mcp__gh__issues、mcp__gh__pulls\n{}", "说明".repeat(200))));
        for i in 0..3 {
            m.push(call(&format!("c{i}"), "a.rs"));
            m.push(result(&format!("c{i}"), "y".repeat(400)));
        }
        clear_old(&mut m);
        let Part::ToolResult { output, .. } = &m[2].parts[0] else { panic!() };
        assert!(output.starts_with(LOADED) && output.contains(CLEARED), "{output}");
        assert_eq!(loaded_in(&m), vec!["mcp__gh__issues".to_string(), "mcp__gh__pulls".to_string()]);
        assert_eq!(spill_large(&mut m, Path::new("/nonexistent"), &AtomicU32::new(0)), 0, "清过的不落盘");
    }

    #[test]
    fn the_summary_follows_the_window_and_the_focus() {
        assert_eq!(summary_target(Some(64_000)), 1500);
        assert_eq!(summary_target(Some(200_000)), 3000);
        assert_eq!(summary_target(Some(1_000_000)), 8000);
        let small = summary_request(Some(64_000), None);
        assert!(small.contains("about 1100 words") && !small.contains("in particular"), "{small}");
        let at: Vec<usize> = ["## 目标", "## 已定的决策", "## 踩过的坑", "## 文件现在的状态", "## 下一步"].iter().map(|h| small.find(h).expect(h)).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "五栏按顺序：{small}");
        let big = summary_request(Some(1_000_000), Some("数据库迁移的步骤"));
        assert!(big.contains("about 6000 words") && big.contains("in particular, in detail: 数据库迁移的步骤"), "{big}");
        assert_eq!(summary_max_output(Some(200_000), 64_000), 16_000);
        assert_eq!(summary_max_output(Some(1_000_000), 64_000), 16_000);
        assert_eq!(summary_max_output(Some(1_000_000), 8_192), 8_192, "不超过模型自己的上限");
    }

    #[test]
    fn every_user_message_is_kept_until_the_budget_then_the_middle_goes() {
        let mut m = sample();
        m.push(Message { role: Role::User, parts: vec![Part::Text { text: "<hook-context event=\"UserPromptSubmit\">\n今天周一\n</hook-context>".into() }] });
        m.push(said("第二句"));
        m.push(said("第三句"));
        let asks = user_requests(&m, Some(200_000));
        assert_eq!(asks.all(), vec!["把标题改成中文".to_string(), "第二句".into(), "第三句".into()]);
        assert_eq!(asks.skipped, 0);
        // 超预算：留第一句和最近的
        let mut long = vec![said("原始目标")];
        for i in 0..30 {
            long.push(said(&format!("第 {i} 句 {}", "字".repeat(400))));
        }
        let asks = user_requests(&long, Some(64_000));
        assert_eq!(asks.first.as_deref(), Some("原始目标"));
        assert!(asks.skipped > 0 && asks.rest.last().unwrap().starts_with("第 29 句"));
        assert_eq!(asks.skipped + asks.rest.len(), 30);
        let new = rebuild("## 目标\n改标题", &asks, &Restore::default());
        let Part::Text { text } = &new[0].parts[1] else { panic!() };
        assert!(text.starts_with(SAID_TAG) && text.contains(&format!("\n\n（中间还有 {} 句，见摘要）\n\n", asks.skipped)), "{text}");
    }

    #[test]
    fn rebuilding_puts_back_the_environment_tools_and_files_in_order() {
        let asks = user_requests(&sample(), Some(200_000));
        let restore = Restore {
            env: Some(format!("{CONTEXT_TAG}\nnow\n</peck-context>")),
            loaded: vec!["mcp__gh__issues".into()],
            files: Some(format!("{RESTORED_TAG}\n=== a.rs ===\n</peck-restored-files>")),
        };
        let new = rebuild("## 目标\n改标题", &asks, &restore);
        assert_eq!(new.len(), 1);
        let texts: Vec<&str> = new[0].parts.iter().map(|p| match p { Part::Text { text } => text.as_str(), _ => "" }).collect();
        assert!(texts[0].starts_with(HANDOFF_TAG));
        assert!(texts[1].starts_with(SAID_TAG) && texts[1].contains("> 把标题改成中文"));
        assert!(texts[2].starts_with(CONTEXT_TAG));
        assert!(texts[3].starts_with(LOADED_TAG));
        assert!(texts[4].starts_with(RESTORED_TAG));
        assert!(texts[5].starts_with(RESUME_TAG) && texts[5].contains("don't need to re-read"));
        assert_eq!(user_requests(&new, Some(200_000)), asks, "再压一次：站主的话从带着的那块认回来，Peck 加的别的不算");
        assert_eq!(loaded_in(&new), vec!["mcp__gh__issues".to_string()], "压缩后还认得回加载过的工具");
    }

    #[test]
    fn compacting_again_keeps_what_the_user_said_before_and_their_answers() {
        // 第一次压缩前：目标、一句多行的话、一次单选的 ASK、一次几个问题的 ASK（长，过了清旧输出的 160 字节）
        let long_answer = format!("{ANSWERED}\n- 用哪个库？：serde\n- 为什么？：{}", "因为".repeat(80));
        let mut m = vec![said("原始目标：把导出改成 CSV"), said("第二句\n\n> 引用的一行")];
        m.push(Message { role: Role::Assistant, parts: vec![Part::ToolCall { call_id: "a1".into(), name: tools::ASK_USER.into(), input: json!({"question": "分隔符？", "options": ["逗号", "分号"]}), signature: None }] });
        m.push(result("a1", format!("{ANSWERED}分号")));
        m.push(Message { role: Role::Assistant, parts: vec![Part::ToolCall { call_id: "a2".into(), name: tools::ASK_USER.into(), input: json!({"questions": [{"question": "用哪个库？"}, {"question": "为什么？"}]}), signature: None }] });
        m.push(result("a2", long_answer.clone()));
        m.push(Message { role: Role::Assistant, parts: vec![Part::ToolCall { call_id: "a3".into(), name: tools::ASK_USER.into(), input: json!({"question": "要不要表头？"}), signature: None }] });
        m.push(result("a3", "站主没回答（任务停了）。".into()));
        for i in 0..4 {
            m.push(call(&format!("c{i}"), &format!("f{i}.rs")));
            m.push(result(&format!("c{i}"), "x".repeat(5000)));
        }
        let dir = std::env::temp_dir().join(format!("peck-compact-{}", uuid::Uuid::new_v4()));
        spill_large(&mut m, &dir, &AtomicU32::new(0));
        clear_old(&mut m);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(m.iter().flat_map(|x| &x.parts).any(|p| matches!(p, Part::ToolResult { output, .. } if *output == long_answer)), "ASK 的回答不清、不落盘");
        let first = user_requests(&m, Some(200_000));
        assert_eq!(first.all(), vec![
            "原始目标：把导出改成 CSV".to_string(),
            "第二句\n\n> 引用的一行".into(),
            "（回答「分隔符？」）分号".into(),
            format!("（回答提问）\n{}", long_answer.strip_prefix(ANSWERED).unwrap().trim()),
        ], "没答的不算");
        // 压完又说了一句、又压一次：前面的都还在，第一句还是原来的目标
        let mut again = rebuild("## 目标\n导出 CSV", &first, &Restore::default());
        again.push(said("第三句"));
        let second = user_requests(&again, Some(200_000));
        assert_eq!(second.all(), [first.all(), vec!["第三句".to_string()]].concat());
        // 超预算省掉的句数跨压缩累加，位置在第一句后面
        let mut long = vec![said("原始目标")];
        for i in 0..30 {
            long.push(said(&format!("第 {i} 句 {}", "字".repeat(400))));
        }
        let cut = user_requests(&long, Some(64_000));
        let mut later = rebuild("摘要", &cut, &Restore::default());
        later.push(said(&format!("第 30 句 {}", "字".repeat(400))));
        let cut2 = user_requests(&later, Some(64_000));
        assert_eq!(cut2.first.as_deref(), Some("原始目标"));
        assert!(cut2.rest.last().unwrap().starts_with("第 30 句"));
        assert_eq!(cut2.skipped + cut2.rest.len(), 31, "{} + {}", cut2.skipped, cut2.rest.len());
    }

    #[test]
    fn recent_files_put_changed_ones_first_then_the_latest() {
        let mut m = vec![said("改一下")];
        let touch = |id: &str, name: &str, input: Value| Message { role: Role::Assistant, parts: vec![Part::ToolCall { call_id: id.into(), name: name.into(), input, signature: None }] };
        m.push(touch("1", "read_file", json!({"path": "a.rs"})));
        m.push(touch("2", "write_patch", json!({"edits": [{"path": "b.rs", "old_text": "x", "new_text": "y"}]})));
        m.push(touch("3", "read_file", json!({"path": "c.rs"})));
        m.push(touch("4", "read_file", json!({"path": "a.rs"})));
        m.push(touch("5", "read_file", json!({"path": "d.rs"})));
        assert_eq!(recent_files(&m, &[]), vec!["b.rs", "d.rs", "a.rs", "c.rs"]);
        assert_eq!(recent_files(&m, &["c.rs".into()]), vec!["c.rs", "b.rs", "d.rs", "a.rs"], "这个任务以前改过的也排前面");
    }

    #[test]
    fn restored_files_are_fresh_capped_trimmed_and_count_as_read() {
        let p = crate::harness::tools::testing::TempProject::new();
        let root = p.dir.clone();
        std::fs::write(root.join("small.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        std::fs::write(root.join("big.rs"), (1..=3000).map(|i| format!("let line_{i} = {i};\n")).collect::<String>()).unwrap();
        std::fs::write(root.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        let ctx = p.ctx();
        let names: Vec<String> = ["gone.rs", "small.rs", "bin.dat", "big.rs", "small.rs"].iter().map(|s| s.to_string()).collect();
        let block = restore_files(&ctx, &names, Some(64_000)).unwrap();
        assert!(block.starts_with(RESTORED_TAG));
        assert!(block.contains("=== small.rs（共 2 行）===\n     1\tfn a() {}"), "{block}");
        assert!(!block.contains("gone.rs") && !block.contains("bin.dat"), "删了的、二进制跳过");
        assert!(block.contains("=== big.rs（共 3000 行，中间第") && block.contains("行省略）") && block.contains("let line_3000"), "放不下的留头尾");
        assert!(context::estimate(&block) <= 6_400 + 200, "不超过窗口的 10%");
        assert_eq!(block.matches("=== small.rs").count(), 1, "同一个文件只放一次");
        assert!(ctx.read_stamp(&root.join("small.rs").canonicalize().unwrap()).is_some(), "放回的登记成读过");
        assert!(restore_files(&ctx, &["gone.rs".into()], Some(64_000)).is_none());
    }

    #[test]
    fn water_filling_gives_small_ones_all_they_need() {
        assert_eq!(water_fill(&[100, 5000, 300], 3000), vec![100, 2600, 300]);
        assert_eq!(water_fill(&[4000, 4000], 3000), vec![1500, 1500]);
    }
}
