//! 顶栏 QUOTA 页的图表数据：从事件日志里取近几十天的花费（Peck 循环、ONE SHOT、CLI 引擎估的）和订阅额度的历史。
//!
//! 只读、只汇总，不另存。额度与花费的新模块（`src/quota/`）是站主自己的作业，这里只是现有用量页的数据来源。

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
}

fn delta_u(now: Option<u64>, before: Option<u64>) -> Option<u64> {
    now.map(|n| n.saturating_sub(before.unwrap_or(0)))
}

/// `rows`：（任务 id, 引擎, 时间, 事件 JSON, 任务的「谁来干」）。
pub fn spend_points(rows: Vec<(String, String, i64, String, Option<String>)>) -> Vec<SpendPoint> {
    let mut last: HashMap<String, Last> = HashMap::new();
    let mut out = Vec::new();
    for (run_id, engine, at, raw, who) in rows {
        let Ok(event) = serde_json::from_str::<Value>(&raw) else { continue };
        let usage = &event["usage"];
        let cost = &event["cost"];
        let (mut usd, mut input, mut output, mut cache_read) =
            (cost["usd"].as_f64(), num(&usage["input"]), num(&usage["output"]), num(&usage["cacheRead"]));
        if event["scope"].as_str() == Some("cumulative") {
            let prev = last.entry(run_id.clone()).or_default();
            let next = Last { usd, input, output, cache_read };
            usd = usd.map(|u| (u - prev.usd.unwrap_or(0.0)).max(0.0));
            input = delta_u(input, prev.input);
            output = delta_u(output, prev.output);
            cache_read = delta_u(cache_read, prev.cache_read);
            *prev = next;
        }
        let key = match who.as_deref().and_then(|w| w.strip_prefix("api:")) {
            Some(model) => model.to_string(),
            None if engine == "peck" => "oneshot".to_string(),
            None => engine.clone(),
        };
        out.push(SpendPoint {
            at,
            engine,
            key,
            usd,
            estimated: cost["source"].as_str() == Some("estimated"),
            input,
            output,
            cache_read,
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
            "SELECT e.run_id, e.engine, e.at, e.event, json_extract(r.record, '$.who')
             FROM agent_events e LEFT JOIN runs r ON r.id = e.run_id
             WHERE e.type = 'usage.updated' AND e.at >= ?1 ORDER BY e.at, e.seq",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
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
    Ok(SpendOverview { since, points: spend_points(rows), quota: quota_points(quota_rows) })
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
        ];
        let points = spend_points(rows);
        assert_eq!(points.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["deepseek/deepseek-flash", "deepseek/deepseek-flash", "claude-code", "claude-code", "oneshot"]);
        assert!((points[3].usd.unwrap() - 0.15).abs() < 1e-9, "累计的取差值");
        assert_eq!(points[3].input, Some(500));
        assert!(points[2].estimated && !points[0].estimated);
        let total: f64 = points.iter().filter_map(|p| p.usd).sum();
        assert!((total - 0.285).abs() < 1e-9);
    }

    #[test]
    fn quota_windows_become_points() {
        let raw = r#"{"type":"quota.updated","provider":"claude-code","windows":[{"label":"5 小时","usedPercent":26.0,"resetsAt":9},{"label":"每周","usedPercent":77.0}]}"#;
        let points = quota_points(vec![("claude-code".into(), 7, raw.into())]);
        assert_eq!(points.len(), 2);
        assert_eq!((points[1].label.as_str(), points[1].used_percent, points[0].resets_at), ("每周", 77.0, Some(9)));
    }
}
