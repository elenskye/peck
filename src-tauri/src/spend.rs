//! 顶栏 QUOTA 页的图表数据：从事件日志里取近几十天的花费（Peck 循环、ONE SHOT、CLI 引擎估的）和订阅额度的历史。
//!
//! 只读、只汇总，不另存。额度与花费的新模块（`src/quota/`）是站主自己的作业，这里只是现有用量页的数据来源。

use crate::gateway::event::TokenUsage;
use crate::gateway::prices::Book;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use tauri::State;

/// 一次请求（或一个回合）花的。按天、按谁汇总由界面做（界面知道本地时区）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpendPoint {
    pub at: i64,
    /// `peck`（Peck 循环和 ONE SHOT）或 CLI 引擎 id。
    pub engine: String,
    /// 谁花的：Peck 循环是目录里的模型 id（`deepseek/deepseek-flash`），ONE SHOT 是 `oneshot`，CLI 是引擎 id。
    pub key: String,
    pub usd: Option<f64>,
    /// CLI 自己估的（不是按价格表算的）。
    pub estimated: bool,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    #[serde(default)]
    pub cache_write: Option<u64>,
}

/// 订阅额度的一个读数（CLI 引擎在任务里报的）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPoint {
    pub at: i64,
    pub engine: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendOverview {
    pub since: i64,
    pub points: Vec<SpendPoint>,
    pub quota: Vec<QuotaPoint>,
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
}

/// 累计口径（Claude Code 续接后）的读数换成这一次的增量。
#[derive(Default)]
struct Last {
    usd: Option<f64>,
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
}

/// 事件日志里的一条用量，带上它所在的任务和 CLI 会话。
pub struct UsageRow {
    pub run_id: String,
    pub engine: String,
    pub at: i64,
    pub raw: String,
    /// 任务的「谁来干」。
    pub who: Option<String>,
    /// CLI 会话（`session.started` 的 `sessionRef`）。
    pub session: Option<String>,
    /// CLI 会话用的模型（`session.started` 的 `model`）：按价格表折算 CLI 的花费。
    pub session_model: Option<String>,
}

fn delta_u(now: Option<u64>, before: Option<u64>) -> Option<u64> {
    now.map(|n| n.saturating_sub(before.unwrap_or(0)))
}

/// `rows`：（任务 id, 引擎, 时间, 事件 JSON, 任务的「谁来干」）。
#[cfg(test)]
pub fn spend_points(rows: Vec<(String, String, i64, String, Option<String>)>) -> Vec<SpendPoint> {
    spend_points_by_session(rows.into_iter().map(|(r, e, a, raw, w)| (r, e, a, raw, w, None)).collect())
}

/// 同上，多一列：这条用量所在的 CLI 会话（`session.started` 的 `sessionRef`）。
/// 累计口径按「引擎 + 会话」取差（v0.10 查花费时补的）：换一个任务续接同一个会话，Claude Code、Codex 报的是整个会话的累计，
/// 以前按任务取差，续接的第一条整个算进去（重复算）。读数比上一次少（CLI 没把前面的接上）就当新的一份。
#[cfg(test)]
pub fn spend_points_by_session(rows: Vec<(String, String, i64, String, Option<String>, Option<String>)>) -> Vec<SpendPoint> {
    let rows = rows
        .into_iter()
        .map(|(run_id, engine, at, raw, who, session)| UsageRow { run_id, engine, at, raw, who, session, session_model: None })
        .collect();
    priced_points(rows, None)
}

/// 同上，再按价格表补上算不出的（v0.10 Refine，站主 11-17）：CLI 按它会话用的模型折成 API 价（订阅不真花，标「估」，
/// 有价格表就不用 CLI 自己估的）；Peck 循环当时没价格的，按现在的价格表补。Peck 循环当时算好的照旧（当时的价格）。
pub fn priced_points(rows: Vec<UsageRow>, book: Option<&Book>) -> Vec<SpendPoint> {
    let mut last: HashMap<String, Last> = HashMap::new();
    let mut out = Vec::new();
    for UsageRow { run_id, engine, at, raw, who, session, session_model } in rows {
        let Ok(event) = serde_json::from_str::<Value>(&raw) else { continue };
        let usage = &event["usage"];
        let cost = &event["cost"];
        let (mut usd, mut input, mut output, mut cache_read, mut cache_write) =
            (cost["usd"].as_f64(), num(&usage["input"]), num(&usage["output"]), num(&usage["cacheRead"]), num(&usage["cacheWrite"]));
        if event["scope"].as_str() == Some("cumulative") {
            let key = session.as_ref().filter(|s| !s.is_empty()).map(|s| format!("{engine}:{s}")).unwrap_or_else(|| run_id.clone());
            let prev = last.entry(key).or_default();
            let next = Last { usd, input, output, cache_read, cache_write };
            let reset = match (input, prev.input) {
                (Some(now), Some(before)) => now < before,
                _ => usd.zip(prev.usd).is_some_and(|(now, before)| now < before),
            };
            if !reset {
                usd = usd.map(|u| (u - prev.usd.unwrap_or(0.0)).max(0.0));
                input = delta_u(input, prev.input);
                output = delta_u(output, prev.output);
                cache_read = delta_u(cache_read, prev.cache_read);
                cache_write = delta_u(cache_write, prev.cache_write);
            }
            *prev = next;
        }
        // v0.9 起 Peck 循环的用量带模型 id（委派的子任务是子任务的模型，合作里是那一段的模型），按它记；
        // 旧记录没有：任务的「谁」是 api: 的记那个模型，合作里分不出的记「合作」，不是任务的（ONE SHOT）记 oneshot。
        let model = event["model"].as_str().filter(|m| !m.is_empty());
        let key = match (model, who.as_deref()) {
            (_, None) if engine == "peck" => "oneshot".to_string(),
            (Some(m), _) => m.to_string(),
            (None, Some(w)) if w.starts_with("api:") && engine == "peck" => w["api:".len()..].to_string(),
            (None, Some(w)) if w.starts_with("team:") && engine == "peck" => "team".to_string(),
            (None, _) if engine == "peck" => "oneshot".to_string(),
            (None, _) => engine.clone(),
        };
        // 带模型 id 的都是 Peck 循环花的（CLI 委派出去的子任务记在 CLI 的任务里，也算 Peck）。
        let cli = model.is_none() && engine != "peck";
        let engine = if model.is_some() { "peck".to_string() } else { engine };
        let mut estimated = cost["source"].as_str() == Some("estimated");
        if let Some(book) = book {
            let tokens = TokenUsage { input, output, cache_read, cache_write, ..Default::default() };
            if cli {
                // CLI 报的是一个回合（或整个会话取差）的合计，不按单次请求分档。
                if let Some(v) = session_model.as_deref().and_then(|m| book.usd_for(m, &tokens, false, at)) {
                    usd = Some(v);
                    estimated = true;
                }
            } else if usd.is_none() {
                if let Some(v) = model.and_then(|m| book.usd_for(m, &tokens, true, at)) {
                    usd = Some(v);
                }
            }
        }
        out.push(SpendPoint {
            at,
            engine,
            key,
            usd,
            estimated,
            input,
            output,
            cache_read,
            cache_write,
        });
    }
    out
}

pub fn quota_points(rows: Vec<(String, i64, String)>) -> Vec<QuotaPoint> {
    let mut out = Vec::new();
    for (engine, at, raw) in rows {
        let Ok(event) = serde_json::from_str::<Value>(&raw) else { continue };
        for w in event["windows"].as_array().into_iter().flatten() {
            if let Some(used) = w["usedPercent"].as_f64() {
                out.push(QuotaPoint {
                    at,
                    engine: engine.clone(),
                    label: w["label"].as_str().unwrap_or("").to_string(),
                    used_percent: used,
                    resets_at: w["resetsAt"].as_i64(),
                });
            }
        }
    }
    out
}

/// QUOTA 页：近 `days` 天的花费和额度读数。
#[tauri::command]
pub fn spend_overview(days: u32, state: State<'_, AppState>) -> Result<SpendOverview, String> {
    let since = crate::store::now_ms() - i64::from(days.clamp(1, 120)) * 86_400_000;
    let db = state.store.lock().map_err(|_| "读不到花费。")?;
    let mut stmt = db
        .conn
        .prepare(
            "SELECT e.run_id, e.engine, e.at, e.event, json_extract(r.record, '$.who'),
                    (SELECT json_extract(s.event, '$.sessionRef') FROM agent_events s
                      WHERE s.run_id = e.run_id AND s.engine = e.engine AND s.type = 'session.started' AND s.seq < e.seq
                        AND json_extract(s.event, '$.subagentId') IS NULL
                      ORDER BY s.seq DESC LIMIT 1),
                    (SELECT json_extract(s.event, '$.model') FROM agent_events s
                      WHERE s.run_id = e.run_id AND s.engine = e.engine AND s.type = 'session.started' AND s.seq < e.seq
                        AND json_extract(s.event, '$.subagentId') IS NULL
                      ORDER BY s.seq DESC LIMIT 1)
             FROM agent_events e LEFT JOIN runs r ON r.id = e.run_id
             WHERE e.type = 'usage.updated' AND e.at >= ?1 ORDER BY e.at, e.seq",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([since], |r| {
            Ok(UsageRow { run_id: r.get(0)?, engine: r.get(1)?, at: r.get(2)?, raw: r.get(3)?, who: r.get(4)?, session: r.get(5)?, session_model: r.get(6)? })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    let mut stmt = db
        .conn
        .prepare("SELECT engine, at, event FROM agent_events WHERE type = 'quota.updated' AND at >= ?1 ORDER BY at")
        .map_err(|e| e.to_string())?;
    let quota_rows = stmt
        .query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    let book = crate::gateway::prices::book();
    Ok(SpendOverview { since, points: priced_points(rows, Some(&book)), quota: quota_points(quota_rows) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_add_up_and_cumulative_readings_become_deltas() {
        let ev = |usd: f64, input: u64, scope: &str, source: &str| {
            format!(r#"{{"type":"usage.updated","usage":{{"input":{input},"output":10}},"cost":{{"usd":{usd},"source":"{source}"}},"scope":"{scope}"}}"#)
        };
        let rows = vec![
            ("r1".into(), "peck".into(), 1, ev(0.01, 100, "turn", "priced"), Some("api:deepseek/deepseek-flash".into())),
            ("r1".into(), "peck".into(), 2, ev(0.02, 200, "turn", "priced"), Some("api:deepseek/deepseek-flash".into())),
            ("r2".into(), "claude-code".into(), 3, ev(0.10, 1000, "cumulative", "estimated"), Some("cli:claude-code".into())),
            ("r2".into(), "claude-code".into(), 4, ev(0.25, 1500, "cumulative", "estimated"), Some("cli:claude-code".into())),
            ("o1".into(), "peck".into(), 5, ev(0.005, 50, "turn", "priced"), None),
            ("t1".into(), "peck".into(), 6, ev(0.002, 20, "turn", "priced"), Some("team:cli:codex|api:deepseek/deepseek-flash|".into())),
        ];
        let points = spend_points(rows);
        assert_eq!(points.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["deepseek/deepseek-flash", "deepseek/deepseek-flash", "claude-code", "claude-code", "oneshot", "team"]);
        // v0.9：带模型 id 的按模型记——合作里那一段、Peck 循环委派的子任务、CLI 委派出去的子任务（记成 Peck）。
        let with = |model: &str| format!(r#"{{"type":"usage.updated","usage":{{"input":10}},"cost":{{"usd":0.001,"source":"priced"}},"scope":"turn","model":"{model}"}}"#);
        let newer = spend_points(vec![
            ("t2".into(), "peck".into(), 7, with("anthropic/claude-sonnet-5-5"), Some("team:|api:anthropic/claude-sonnet-5-5|cli:codex".into())),
            ("p2".into(), "peck".into(), 8, with("deepseek/deepseek-flash"), Some("api:anthropic/claude-sonnet-5-5".into())),
            ("c2".into(), "claude-code".into(), 9, with("deepseek/deepseek-flash"), Some("cli:claude-code".into())),
            ("o2".into(), "peck".into(), 10, with("deepseek/deepseek-flash"), None),
        ]);
        assert_eq!(newer.iter().map(|p| (p.key.as_str(), p.engine.as_str())).collect::<Vec<_>>(), [
            ("anthropic/claude-sonnet-5-5", "peck"),
            ("deepseek/deepseek-flash", "peck"),
            ("deepseek/deepseek-flash", "peck"),
            ("oneshot", "peck"),
        ]);
        assert!((points[3].usd.unwrap() - 0.15).abs() < 1e-9, "累计的取差值");
        assert_eq!(points[3].input, Some(500));
        assert!(points[2].estimated && !points[0].estimated);
        let total: f64 = points.iter().filter_map(|p| p.usd).sum();
        assert!((total - 0.287).abs() < 1e-9);
    }

    #[test]
    fn a_resumed_cli_session_is_not_counted_twice_across_tasks() {
        let ev = |usd: f64, input: u64| format!(r#"{{"type":"usage.updated","usage":{{"input":{input}}},"cost":{{"usd":{usd},"source":"estimated"}},"scope":"cumulative"}}"#);
        let who = || Some("cli:claude-code".to_string());
        let points = spend_points_by_session(vec![
            ("r1".into(), "claude-code".into(), 1, ev(0.10, 1000), who(), Some("s1".into())),
            // 另一个任务续接 s1：报的是整个会话的累计。
            ("r2".into(), "claude-code".into(), 2, ev(0.25, 1500), who(), Some("s1".into())),
            // 交回去修（同一个任务、同一个会话）。
            ("r2".into(), "claude-code".into(), 3, ev(0.30, 1700), who(), Some("s1".into())),
            // 新会话：从头算。
            ("r3".into(), "claude-code".into(), 4, ev(0.05, 300), who(), Some("s2".into())),
            // 读数变少了（CLI 没接上前面的）：当新的一份。
            ("r4".into(), "claude-code".into(), 5, ev(0.02, 200), who(), Some("s1".into())),
        ]);
        let usd: Vec<f64> = points.iter().map(|p| (p.usd.unwrap() * 100.0).round() / 100.0).collect();
        assert_eq!(usd, vec![0.10, 0.15, 0.05, 0.05, 0.02]);
        assert_eq!(points[1].input, Some(500));
    }

    #[test]
    fn cli_usage_is_priced_by_the_session_model() {
        let book = crate::gateway::prices::book();
        let ev = |input: u64, scope: &str, usd: &str| format!(r#"{{"type":"usage.updated","usage":{{"input":{input},"output":1000,"cacheRead":0}},"cost":{{"usd":{usd},"source":"unknown"}},"scope":"{scope}"}}"#);
        let row = |run: &str, engine: &str, raw: String, model: Option<&str>| UsageRow {
            run_id: run.into(), engine: engine.into(), at: 1, raw, who: Some(format!("cli:{engine}")), session: Some("s".into()), session_model: model.map(str::to_string),
        };
        let points = priced_points(vec![
            row("r1", "codex", ev(1_000_000, "cumulative", "null"), Some("gpt-6-luna")),
            row("r1", "codex", ev(3_000_000, "cumulative", "null"), Some("gpt-6-luna")),
            // 模型没在价格表里：照旧算不出。
            row("r2", "muse", ev(10, "turn", "null"), Some("no-such-model")),
            row("r3", "antigravity", ev(1_000_000, "turn", "null"), Some("gemini-4-argon-medium")),
        ], Some(&book));
        let luna = book.find("gpt-6-luna").unwrap();
        // 第二条取差：200 万进、0 出（累计的 output 没变）。
        let want = 2.0 * luna.input;
        assert!((points[1].usd.unwrap() - want).abs() < 1e-9, "{:?}", points[1]);
        assert!(points[0].estimated && points[1].estimated);
        assert_eq!(points[2].usd, None);
        assert!(points[3].usd.unwrap() > 0.0);
        // 没给价格表：和以前一样。
        assert_eq!(priced_points(vec![row("r1", "codex", ev(10, "turn", "null"), Some("gpt-6-luna"))], None)[0].usd, None);
    }

    #[test]
    fn quota_windows_become_points() {
        let raw = r#"{"type":"quota.updated","provider":"claude-code","windows":[{"label":"5 小时","usedPercent":26.0,"resetsAt":9},{"label":"每周","usedPercent":77.0}]}"#;
        let points = quota_points(vec![("claude-code".into(), 7, raw.into())]);
        assert_eq!(points.len(), 2);
        assert_eq!((points[1].label.as_str(), points[1].used_percent, points[0].resets_at), ("每周", 77.0, Some(9)));
    }
}
