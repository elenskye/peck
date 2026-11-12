//! 原件 → 统一事件。每家一个转换器，夹具测试和真引擎用同一份代码：
//! 真引擎（Phase 5 M2）一行一行喂 stdout，夹具测试喂录下来的 `raw.jsonl`，结果和 `events.jsonl` 比。
//!
//! 不认识的原件变成 `raw` 事件，不报错、不丢。

use super::event::EventBody;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 转换器出的一条：事件本体，和它属于哪个子任务（主线为空）。
/// 就是 `AgentEvent` 去掉 `seq`、`at`；夹具的 `events.jsonl` 一行一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Emitted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,
    #[serde(flatten)]
    pub body: EventBody,
}

impl From<EventBody> for Emitted {
    fn from(body: EventBody) -> Self {
        Self {
            subagent_id: None,
            body,
        }
    }
}

/// 一行原件（JSON）进，零到多条统一事件出。
pub trait LineConverter {
    fn feed(&mut self, raw: &Value) -> Vec<Emitted>;
    /// 原件读完了（进程退出）。补上没收尾的事件。
    fn finish(&mut self) -> Vec<Emitted> {
        Vec::new()
    }
}

/// 把一整份 JSONL 原件转完（夹具测试用）。读不懂的行变成 `raw`。
pub fn convert_jsonl(converter: &mut dyn LineConverter, source: &str, jsonl: &str) -> Vec<Emitted> {
    let mut out = Vec::new();
    for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => out.extend(converter.feed(&value)),
            Err(_) => out.push(
                EventBody::Raw {
                    source: source.into(),
                    kind: Some("unparsed".into()),
                }
                .into(),
            ),
        }
    }
    out.extend(converter.finish());
    out
}

/// 原件里的类型名（`type`、`method` 之类），给 `raw` 事件用。
pub(crate) fn raw_kind(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| value.get(*k).and_then(Value::as_str))
        .map(str::to_string)
}

/// 把整份 unified diff 按文件拆开，数加减行。`root` 是项目根（引擎给的是绝对路径时去掉它）。
pub fn split_unified_diff(diff: &str, root: Option<&str>) -> Vec<super::event::FileDiff> {
    use super::event::{FileChange, FileDiff};
    let mut out: Vec<FileDiff> = Vec::new();
    let mut current: Option<(FileDiff, String)> = None;
    let flush = |current: &mut Option<(FileDiff, String)>, out: &mut Vec<FileDiff>| {
        if let Some((mut file, text)) = current.take() {
            file.patch = Some(text);
            out.push(file);
        }
    };
    for line in diff.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&mut current, &mut out);
            let rest = rest.trim_end();
            let (a, b) = rest.split_once(" b/").unwrap_or((rest, rest));
            let old = relative(a.trim_start_matches("a/"), root);
            let new = relative(b, root);
            current = Some((
                FileDiff {
                    change: if old != new { FileChange::Renamed } else { FileChange::Modified },
                    old_path: (old != new).then_some(old),
                    path: new,
                    additions: Some(0),
                    deletions: Some(0),
                    patch: None,
                },
                String::new(),
            ));
        }
        let Some((file, text)) = current.as_mut() else {
            continue;
        };
        text.push_str(line);
        if line.starts_with("new file mode") {
            file.change = FileChange::Added;
        } else if line.starts_with("deleted file mode") {
            file.change = FileChange::Deleted;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            *file.additions.get_or_insert(0) += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            *file.deletions.get_or_insert(0) += 1;
        }
    }
    flush(&mut current, &mut out);
    out
}

/// 绝对路径去掉项目根，变成相对路径；本来就是相对的原样返回。
pub fn relative(path: &str, root: Option<&str>) -> String {
    match root {
        Some(root) if !root.is_empty() => path
            .strip_prefix(root)
            .map(|p| p.trim_start_matches('/'))
            .filter(|p| !p.is_empty())
            .unwrap_or(path)
            .to_string(),
        _ => path.to_string(),
    }
}

/// 一行里数加减行（单个文件的 diff）。
pub fn count_lines(diff: &str) -> (u32, u32) {
    let mut add = 0;
    let mut del = 0;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            add += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            del += 1;
        }
    }
    (add, del)
}

/// 截一段给人看的短标题（命令、路径太长时）。
pub fn short(text: &str, max: usize) -> String {
    let one_line = text.lines().next().unwrap_or("").trim();
    if one_line.chars().count() <= max {
        one_line.to_string()
    } else {
        let mut s: String = one_line.chars().take(max).collect();
        s.push('…');
        s
    }
}

pub mod acp;
pub mod anthropic;
pub mod chat;
pub mod claude_code;
pub mod codex;
pub mod gemini;
pub mod muse;
pub mod responses;

// ───────────────────────── 裸模型的流 ─────────────────────────

use super::event::{Cost, TokenUsage};
use super::{ModelEvent, ModelRequest, ModelResponse, Part};

/// 一家 API 的一次流式回答：一条一条喂 SSE 的 `data:`（已解析成 JSON），`finish` 拿整条回答。
/// 四种线协议各一个（`anthropic`、`responses`、`chat`、`gemini`），夹具测试和真请求用同一份。
pub trait WireStream: Send {
    fn feed(&mut self, data: &Value) -> Vec<ModelEvent>;
    /// 见到结束标记了，可以不再读（Gemini 没有结束标记，读到流尾为止）。
    fn done(&self) -> bool {
        false
    }
    /// 流读完：整条回答。流里报过错、没有完成原因（中途断了）、工具参数不完整，返回 `Err`（一句中文）。
    fn finish(self: Box<Self>) -> Result<ModelResponse, String>;
}

/// 按目录里的价格算花费；没这个模型或没价格：未知。
pub(crate) fn priced(catalog_id: Option<&str>, region: Option<&str>, usage: &TokenUsage) -> Cost {
    catalog_id
        .and_then(|id| super::catalog::catalog().model(id))
        .map(|m| m.cost(usage, region))
        .unwrap_or_else(Cost::unknown)
}

/// 系统提示拼成一段（不分块的线协议用）。
pub(crate) fn system_text(request: &ModelRequest) -> String {
    request
        .system
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 工具调用 id → 工具名（Gemini 的 `functionResponse` 要带名字）。
pub(crate) fn tool_names(request: &ModelRequest) -> std::collections::HashMap<String, String> {
    request
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .filter_map(|p| match p {
            Part::ToolCall { call_id, name, .. } => Some((call_id.clone(), name.clone())),
            _ => None,
        })
        .collect()
}

/// `a - b`，两个都有才算；结果不为负。
pub(crate) fn minus(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    a.map(|a| a.saturating_sub(b.unwrap_or(0)))
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! 夹具测试：`raw.jsonl` → 转换器 → 和 `events.jsonl` 比。
    //! 改了转换器、确认新结果是对的：`PECK_REGEN_FIXTURES=1 cargo test gateway` 重写 `events.jsonl`。
    use super::Emitted;
    use std::path::PathBuf;

    pub fn dir(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/gateway")
            .join(name)
    }

    pub fn read(name: &str, file: &str) -> String {
        std::fs::read_to_string(dir(name).join(file)).unwrap_or_default()
    }

    pub fn check(name: &str, events: &[Emitted]) {
        let actual: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        if std::env::var_os("PECK_REGEN_FIXTURES").is_some() {
            std::fs::write(dir(name).join("events.jsonl"), &actual).unwrap();
            return;
        }
        let expected = read(name, "events.jsonl");
        let parsed = crate::gateway::mock::parse_events_jsonl(&expected).expect("events.jsonl 读不懂");
        assert_eq!(parsed.len(), events.len(), "{name}：事件条数不一样");
        for (i, (want, got)) in parsed.iter().zip(events).enumerate() {
            assert_eq!(want, got, "{name}：第 {} 条不一样", i + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::FileChange;

    #[test]
    fn unified_diff_is_split_per_file_with_counts() {
        let diff = "diff --git a/src/a.ts b/src/a.ts\nindex 1..2 100644\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,2 +1,2 @@\n-old\n+new\n+more\n\
diff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hi\n\
diff --git a/x.txt b/y.txt\nsimilarity index 100%\nrename from x.txt\nrename to y.txt\n";
        let files = split_unified_diff(diff, None);
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].path.as_str(), files[0].additions, files[0].deletions), ("src/a.ts", Some(2), Some(1)));
        assert_eq!(files[1].change, FileChange::Added);
        assert_eq!(files[2].change, FileChange::Renamed);
        assert_eq!(files[2].old_path.as_deref(), Some("x.txt"));
        assert!(files[0].patch.as_deref().unwrap().starts_with("diff --git a/src/a.ts"));
    }

    #[test]
    fn paths_become_relative_to_the_project() {
        assert_eq!(relative("/ws/src/a.ts", Some("/ws")), "src/a.ts");
        assert_eq!(relative("src/a.ts", Some("/ws")), "src/a.ts");
        assert_eq!(relative("/other/a.ts", Some("/ws")), "/other/a.ts");
        assert_eq!(short("a very long command line", 6), "a very…");
    }
}
