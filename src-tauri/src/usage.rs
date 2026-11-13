use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, State};

pub const CLIENTS: [&str; 5] = ["codex", "claude-code", "antigravity", "muse-code", "grok"];

pub fn client_label(client: &str) -> &'static str {
    match client {
        "codex" => "Codex",
        "claude-code" => "Claude Code",
        "antigravity" => "Antigravity",
        "muse-code" => "Muse",
        "grok" => "Grok Build",
        _ => "未知客户端",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageTokens {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCall {
    pub source: String,
    pub request_id: String,
    pub project: String,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub round: Option<i64>,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub role: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub budget_counted: bool,
    pub usage: Option<UsageTokens>,
    pub currency: Option<String>,
    pub actual_cost: Option<f64>,
    pub estimated_cost: Option<f64>,
    pub price_version: Option<String>,
    pub local_id: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageFilter {
    pub mode: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenSummary {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_creation: Option<i64>,
    pub reasoning: Option<i64>,
    pub unknown_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub currency: Option<String>,
    pub actual: Option<f64>,
    pub estimated: Option<f64>,
    pub actual_unknown_count: i64,
    pub estimated_unknown_count: i64,
    pub price_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub request_count: i64,
    pub tokens: TokenSummary,
    pub costs: Vec<CostSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageListResult {
    pub calls: Vec<UsageCall>,
    pub summary: UsageSummary,
    pub projects: Vec<String>,
    pub models: Vec<String>,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: i64,
    pub skipped: i64,
    pub invalid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    #[serde(default)]
    pub remaining_amount: Option<f64>,
    pub reset_at_ms: Option<i64>,
    pub window_duration_mins: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub account_id: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub source: String,
    pub captured_at: Option<i64>,
    pub manual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCard {
    pub client: String,
    pub label: String,
    pub status: String,
    pub source: String,
    pub coverage: String,
    pub account_id: Option<String>,
    #[serde(default)]
    pub account_label: Option<String>,
    pub tier: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub raw_label: String,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub failure_reason: Option<String>,
    pub collected_at: i64,
    pub fresh: bool,
    pub running: bool,
    pub installed: bool,
    pub manual: bool,
    #[serde(default)]
    pub previous: Option<QuotaSnapshot>,
}

impl ClientCard {
    fn base(client: &str) -> Self {
        let (source, coverage) = client_source_coverage(client);
        Self {
            client: client.into(),
            label: client_label(client).into(),
            status: "not_running".into(),
            source: source.into(),
            coverage: coverage.into(),
            account_id: None,
            account_label: None,
            tier: None,
            windows: vec![],
            raw_label: String::new(),
            last_success_at: None,
            last_failure_at: None,
            failure_reason: None,
            collected_at: now_ms(),
            fresh: false,
            running: false,
            installed: false,
            manual: false,
            previous: None,
        }
    }
}

pub fn client_source_coverage(client: &str) -> (&'static str, &'static str) {
    match client {
        "muse-code" => (
            "muse usage --json（schemaVersion 1）",
            "Muse 订阅窗口与周窗口",
        ),
        "grok" => ("grok --no-auto-update usage --json", "Grok Build 订阅额度"),
        "codex" => (
            "Codex app-server account/rateLimits/read",
            "Codex 默认配置账号的订阅窗口；其他配置或窗口的账号需另行核对",
        ),
        "claude-code" => (
            "Claude Code /usage（非交互，不进模型）",
            "Claude 订阅的 5 小时和每周窗口（桌面 App 和 Claude Code 共用）",
        ),
        "antigravity" => (
            "Antigravity CLI statusline quota（事件驱动）",
            "Antigravity CLI quota bucket；桌面 App 不等于 CLI",
        ),
        _ => ("未知来源", "未知覆盖范围"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: Option<String>,
    pub topped_up_balance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceCard {
    pub provider: String,
    pub label: String,
    pub status: String,
    pub balances: Vec<BalanceInfo>,
    pub checked_at: Option<i64>,
    pub message: String,
    #[serde(default)]
    pub last_success_at: Option<i64>,
    #[serde(default)]
    pub last_failure_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualSnapshotInput {
    pub client: String,
    pub used_percent: Option<f64>,
    pub reset_at_ms: Option<i64>,
    pub note: String,
}

// ---------- 台账：纯汇总与导入解析（可独立测试） ----------

pub fn summarize(calls: &[UsageCall]) -> UsageSummary {
    let mut tokens = TokenSummary::default();
    let add = |slot: &mut Option<i64>, v: Option<i64>| {
        if let Some(v) = v {
            *slot = Some(slot.unwrap_or(0) + v);
        }
    };
    for c in calls {
        match &c.usage {
            None => tokens.unknown_count += 1,
            Some(u) => {
                if u.input_tokens.is_none()
                    || u.output_tokens.is_none()
                    || u.cache_read_tokens.is_none()
                    || u.cache_creation_tokens.is_none()
                    || u.reasoning_tokens.is_none()
                {
                    tokens.unknown_count += 1;
                }
                add(&mut tokens.input, u.input_tokens);
                add(&mut tokens.output, u.output_tokens);
                add(&mut tokens.cache_read, u.cache_read_tokens);
                add(&mut tokens.cache_creation, u.cache_creation_tokens);
                add(&mut tokens.reasoning, u.reasoning_tokens);
            }
        }
    }
    let mut groups: Vec<CostSummary> = vec![];
    for c in calls {
        let idx = groups
            .iter()
            .position(|g: &CostSummary| g.currency == c.currency);
        let idx = match idx {
            Some(i) => i,
            None => {
                groups.push(CostSummary {
                    currency: c.currency.clone(),
                    actual: None,
                    estimated: None,
                    actual_unknown_count: 0,
                    estimated_unknown_count: 0,
                    price_versions: vec![],
                });
                groups.len() - 1
            }
        };
        let g = &mut groups[idx];
        match c.actual_cost {
            Some(v) => g.actual = Some(g.actual.unwrap_or(0.0) + v),
            None => g.actual_unknown_count += 1,
        }
        match c.estimated_cost {
            Some(v) => g.estimated = Some(g.estimated.unwrap_or(0.0) + v),
            None => g.estimated_unknown_count += 1,
        }
        if let Some(p) = &c.price_version {
            if !g.price_versions.contains(p) {
                g.price_versions.push(p.clone());
            }
        }
    }
    UsageSummary {
        request_count: calls.len() as i64,
        tokens,
        costs: groups,
    }
}

pub fn parse_import(json: &str) -> Result<Vec<UsageCall>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("导入文件不是有效 JSON：{e}"))?;
    if v.get("schemaVersion").and_then(Value::as_i64) != Some(1) {
        return Err("导入文件 schemaVersion 必须为 1。".into());
    }
    if v.get("source").and_then(Value::as_str) != Some("peck") {
        return Err("导入文件 source 必须为 peck。".into());
    }
    let arr = v
        .get("calls")
        .and_then(Value::as_array)
        .ok_or("导入文件缺少 calls 数组。")?;
    let mut out = vec![];
    for item in arr {
        if let Some(c) = parse_import_call(item) {
            out.push(c);
        }
    }
    Ok(out)
}

pub fn count_invalid(json: &str) -> i64 {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return 0;
    };
    let Some(arr) = v.get("calls").and_then(Value::as_array) else {
        return 0;
    };
    arr.iter()
        .filter(|i| parse_import_call(i).is_none())
        .count() as i64
}

fn opt_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}
fn opt_f64(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}
fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn parse_import_call(item: &Value) -> Option<UsageCall> {
    let source = opt_str(item, "source")?;
    if source != "peck" {
        return None;
    }
    let mode = opt_str(item, "mode")?;
    if mode != "real" && mode != "mock" {
        return None;
    }
    let model = opt_str(item, "model").filter(|s| !s.trim().is_empty())?;
    let request_id = opt_str(item, "requestId").filter(|s| !s.trim().is_empty())?;
    let usage = item
        .get("usage")
        .filter(|u| !u.is_null())
        .map(|u| UsageTokens {
            input_tokens: opt_i64(u, "inputTokens"),
            output_tokens: opt_i64(u, "outputTokens"),
            cache_read_tokens: opt_i64(u, "cacheReadTokens"),
            cache_creation_tokens: opt_i64(u, "cacheCreationTokens"),
            reasoning_tokens: opt_i64(u, "reasoningTokens"),
        });
    Some(UsageCall {
        source,
        request_id,
        project: opt_str(item, "project").unwrap_or_default(),
        run_id: opt_str(item, "runId"),
        task_id: opt_str(item, "taskId"),
        round: opt_i64(item, "round"),
        mode,
        provider: opt_str(item, "provider").unwrap_or_default(),
        model,
        role: opt_str(item, "role").unwrap_or_default(),
        started_at: opt_i64(item, "startedAt").unwrap_or(0),
        finished_at: opt_i64(item, "finishedAt"),
        status: opt_str(item, "status").unwrap_or_default(),
        budget_counted: item
            .get("budgetCounted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        usage,
        currency: opt_str(item, "currency"),
        actual_cost: opt_f64(item, "actualCost"),
        estimated_cost: opt_f64(item, "estimatedCost"),
        price_version: opt_str(item, "priceVersion"),
        local_id: false,
    })
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn build_csv(calls: &[UsageCall]) -> String {
    let mut out = String::new();
    out.push_str("source,requestId,project,runId,taskId,round,mode,provider,model,role,startedAt,finishedAt,status,budgetCounted,inputTokens,outputTokens,cacheReadTokens,cacheCreationTokens,reasoningTokens,currency,actualCost,estimatedCost,priceVersion\n");
    for c in calls {
        let u = c.usage.as_ref();
        let num = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
        let cost = |v: Option<f64>| v.map(|x| x.to_string()).unwrap_or_default();
        let row = [
            c.source.clone(),
            c.request_id.clone(),
            c.project.clone(),
            c.run_id.clone().unwrap_or_default(),
            c.task_id.clone().unwrap_or_default(),
            num(c.round),
            c.mode.clone(),
            c.provider.clone(),
            c.model.clone(),
            c.role.clone(),
            c.started_at.to_string(),
            num(c.finished_at),
            c.status.clone(),
            c.budget_counted.to_string(),
            num(u.and_then(|x| x.input_tokens)),
            num(u.and_then(|x| x.output_tokens)),
            num(u.and_then(|x| x.cache_read_tokens)),
            num(u.and_then(|x| x.cache_creation_tokens)),
            num(u.and_then(|x| x.reasoning_tokens)),
            c.currency.clone().unwrap_or_default(),
            cost(c.actual_cost),
            cost(c.estimated_cost),
            c.price_version.clone().unwrap_or_default(),
        ];
        out.push_str(
            &row.iter()
                .map(|s| csv_cell(s))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

// ---------- 五家解析器（纯函数，输入为官方 JSON） ----------

pub fn parse_muse_usage(v: &Value, collected_at: i64) -> ClientCard {
    let mut card = ClientCard::base("muse-code");
    card.installed = true;
    card.running = true;
    card.collected_at = collected_at;
    let allowance = v.get("allowance");
    let tier = allowance.and_then(|a| opt_str(a, "tier"));
    card.tier = tier.clone();
    let mut windows = vec![];
    if let Some(a) = allowance {
        if let Some(w) = a.get("window") {
            windows.push(QuotaWindow {
                label: match &tier {
                    Some(t) => format!("当前窗口 · {t}"),
                    None => "当前窗口".into(),
                },
                used_percent: opt_f64(w, "usedPercent"),
                remaining_amount: None,
                reset_at_ms: opt_i64(w, "resetsAtMs"),
                window_duration_mins: None,
            });
        }
        if let Some(w) = a.get("weekly") {
            windows.push(QuotaWindow {
                label: "周窗口".into(),
                used_percent: opt_f64(w, "usedPercent"),
                remaining_amount: None,
                reset_at_ms: opt_i64(w, "resetsAtMs"),
                window_duration_mins: Some(7 * 24 * 60),
            });
        }
    }
    if windows.iter().all(|w| w.used_percent.is_none()) {
        card.status = "fields_missing".into();
        card.failure_reason =
            Some("Muse usage 已返回，但 allowance 窗口缺少 usedPercent；未知不填 0。".into());
        card.last_failure_at = Some(collected_at);
        return card;
    }
    card.windows = windows;
    card.account_id = opt_str(v, "accountId");
    card.status = "ok".into();
    card.last_success_at = allowance.and_then(|a| opt_i64(a, "asOfMs"));
    card.fresh = card
        .last_success_at
        .is_some_and(|t| (0..90_000).contains(&(collected_at - t)));
    card.raw_label = format!(
        "官方字段 allowance.window/weekly.usedPercent（0–100）与 resetsAtMs；source={}",
        allowance
            .and_then(|a| opt_str(a, "source"))
            .unwrap_or_else(|| "muse usage（来源未提供）".into())
    );
    card
}

pub fn parse_codex_rate_limits(account: &Value, rate: &Value, collected_at: i64) -> ClientCard {
    let mut card = ClientCard::base("codex");
    card.installed = true;
    card.running = true;
    card.collected_at = collected_at;
    let account_type = opt_str(account, "type");
    card.account_id = opt_str(rate, "accountId").or_else(|| opt_str(account, "accountId"));
    card.account_label = opt_str(account, "email");
    if account_type.as_deref() == Some("apiKey") {
        card.status = "not_logged_in".into();
        card.failure_reason =
            Some("Codex 当前为 API Key 认证，不能用 API Key 推断 ChatGPT 订阅额度。".into());
        card.last_failure_at = Some(collected_at);
        return card;
    }
    if account.get("account").is_none() && account_type.is_none() && card.account_id.is_none() {
        // account/read may wrap account object; handled by caller passing inner value when present.
    }
    let mut windows: Vec<QuotaWindow> = vec![];
    if let Some(map) = rate.get("rateLimitsByLimitId").and_then(Value::as_object) {
        for (limit_id, limit) in map {
            let limit_name = opt_str(limit, "limitName").unwrap_or_else(|| limit_id.clone());
            collect_codex_windows(limit, &limit_name, &mut windows);
        }
    }
    // Deliberately ignore compatible top-level `rateLimits` to avoid double counting the same pool.
    if windows.is_empty() {
        card.status = "fields_missing".into();
        card.failure_reason = Some(
            "Codex app-server 已响应，但 rateLimitsByLimitId 未包含 usedPercent 窗口；未知不填 0。"
                .into(),
        );
        card.last_failure_at = Some(collected_at);
        return card;
    }
    windows.sort_by_key(|w| {
        (
            w.window_duration_mins.unwrap_or(0),
            w.reset_at_ms.unwrap_or(0),
        )
    });
    card.windows = windows;
    card.status = "ok".into();
    card.fresh = true;
    card.last_success_at = Some(collected_at);
    card.raw_label = "官方字段 rateLimitsByLimitId.*.usedPercent / windowDurationMins / resetsAt（秒级时间戳转毫秒）；未重复计算兼容 rateLimits".into();
    card
}

fn collect_codex_windows(v: &Value, label: &str, out: &mut Vec<QuotaWindow>) {
    if let Some(obj) = v.as_object() {
        if let Some(used) = obj.get("usedPercent").and_then(Value::as_f64) {
            let resets = obj.get("resetsAt").and_then(Value::as_i64).map(|s| {
                if s < 10_000_000_000 {
                    s * 1000
                } else {
                    s
                }
            });
            out.push(QuotaWindow {
                label: label.to_string(),
                used_percent: Some(used),
                remaining_amount: None,
                reset_at_ms: resets,
                window_duration_mins: obj.get("windowDurationMins").and_then(Value::as_i64),
            });
        }
        for (k, child) in obj {
            if k == "usedPercent" || k == "resetsAt" || k == "windowDurationMins" {
                continue;
            }
            let child_label = if k == "primary" || k == "secondary" {
                format!("{label} · {k}")
            } else if child.is_object() && child.get("usedPercent").is_some() {
                format!("{label} · {k}")
            } else {
                label.to_string()
            };
            collect_codex_windows(child, &child_label, out);
        }
    } else if let Some(arr) = v.as_array() {
        for child in arr {
            collect_codex_windows(child, label, out);
        }
    }
}

pub fn parse_claude_statusline(v: &Value, collected_at: i64) -> ClientCard {
    let mut card = ClientCard::base("claude-code");
    card.installed = true;
    card.running = true;
    card.collected_at = collected_at;
    let rl = v.get("rate_limits");
    let mut windows = vec![];
    if let Some(rl) = rl {
        for (key, label) in [("five_hour", "5 小时窗口"), ("seven_day", "7 天窗口")] {
            if let Some(w) = rl.get(key) {
                windows.push(QuotaWindow {
                    label: label.into(),
                    used_percent: opt_f64(w, "used_percentage"),
                    remaining_amount: None,
                    reset_at_ms: opt_i64(w, "resets_at").map(|s| {
                        if s < 10_000_000_000 {
                            s * 1000
                        } else {
                            s
                        }
                    }),
                    window_duration_mins: if key == "five_hour" {
                        Some(300)
                    } else {
                        Some(10080)
                    },
                });
            }
        }
    }
    if windows.iter().all(|w| w.used_percent.is_none()) {
        card.status = "fields_missing".into();
        card.failure_reason = Some("Claude Code status line 事件缺少 rate_limits.used_percentage；该字段仅在支持账户且已有模型响应后可能出现，缺失不等于 0。桌面 App 与 CLI 分开，桌面存在不算 CLI 读取。".into());
        card.last_failure_at = Some(collected_at);
        return card;
    }
    card.windows = windows
        .into_iter()
        .filter(|w| w.used_percent.is_some())
        .collect();
    card.status = "ok".into();
    card.fresh = false;
    card.last_success_at = Some(collected_at);
    card.raw_label = "官方 status line 字段 rate_limits.five_hour/seven_day.used_percentage 与 resets_at；事件驱动，可能不新鲜".into();
    card
}

pub fn parse_antigravity_quota(v: &Value, collected_at: i64) -> ClientCard {
    let mut card = ClientCard::base("antigravity");
    card.installed = true;
    card.running = true;
    card.collected_at = collected_at;
    let quota = v.get("quota");
    let mut windows = vec![];
    if let Some(q) = quota.and_then(Value::as_object) {
        for (bucket, item) in q {
            let remaining_fraction = opt_f64(item, "remaining_fraction");
            windows.push(QuotaWindow {
                label: format!("quota · {bucket}"),
                used_percent: remaining_fraction.map(|r| (100.0 - r * 100.0).clamp(0.0, 100.0)),
                remaining_amount: opt_f64(item, "remaining_amount"),
                reset_at_ms: opt_str(item, "reset_time").and_then(|s| parse_reset_time(&s)),
                window_duration_mins: None,
            });
        }
    }
    if windows
        .iter()
        .all(|w| w.used_percent.is_none() && w.remaining_amount.is_none())
    {
        card.status = "fields_missing".into();
        card.failure_reason = Some("Antigravity CLI 事件缺少 quota.remaining_fraction/remaining_amount；上下文比例不是订阅额度，桌面 App 不等于 CLI。".into());
        card.last_failure_at = Some(collected_at);
        return card;
    }
    card.windows = windows
        .into_iter()
        .filter(|w| w.used_percent.is_some() || w.remaining_amount.is_some())
        .collect();
    card.status = "ok".into();
    card.fresh = false;
    card.last_success_at = Some(collected_at);
    card.raw_label = "官方 CLI quota 的 remaining_fraction 或 remaining_amount（供应商额度单位，非货币）；reset_time 保留时区语义".into();
    card
}

fn parse_reset_time(s: &str) -> Option<i64> {
    if let Ok(n) = s.parse::<i64>() {
        return Some(if n < 10_000_000_000 { n * 1000 } else { n });
    }
    let parsed =
        time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

// ---------- 进程识别（仅 pid/ppid/可执行路径，不读 argv/环境） ----------

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: i64,
    pub ppid: i64,
    pub comm: String,
}

pub fn classify_process(comm: &str) -> Option<(&'static str, &'static str)> {
    let lower = comm.to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if lower.contains("muse.app") || base == "muse" || base.starts_with("muse-bin") {
        return Some(("muse-code", "cli-or-app"));
    }
    if base == "grok" || base.starts_with("grok-") {
        return Some(("grok", "cli"));
    }
    if base == "codex" {
        return Some(("codex", "cli-or-app"));
    }
    if lower.contains("claude.app") {
        // 桌面 App 的主进程算「在跑」：它和 Claude Code 用同一份订阅额度。辅助进程不算。
        return (base == "claude").then_some(("claude-code", "desktop"));
    }
    if base == "claude" || lower.contains("/.local/share/claude/versions/") {
        return Some(("claude-code", "cli"));
    }
    if base == "antigravity" {
        let variant = if lower.contains(".app") {
            "desktop"
        } else {
            "cli"
        };
        return Some(("antigravity", variant));
    }
    if base == "chatgpt" && lower.contains("chatgpt.app") {
        return Some(("codex", "desktop"));
    }
    None
}

pub fn running_clients(procs: &[ProcInfo], self_pid: i64) -> HashMap<String, String> {
    let by_pid: HashMap<i64, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut excluded: HashSet<i64> = HashSet::new();
    excluded.insert(self_pid);
    // Exclude Peck subtree: descendants of self.
    loop {
        let mut added = false;
        for p in procs {
            if excluded.contains(&p.ppid) && !excluded.contains(&p.pid) {
                excluded.insert(p.pid);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    // Also exclude ancestors' other? No: user clients are not Peck descendants normally.
    let _ = by_pid;
    let mut out: HashMap<String, String> = HashMap::new();
    for p in procs {
        if excluded.contains(&p.pid) {
            continue;
        }
        let base = p.comm.rsplit('/').next().unwrap_or(&p.comm).to_lowercase();
        if base == "terminal"
            || base == "iterm2"
            || base == "login"
            || base == "zsh"
            || base == "bash"
        {
            continue;
        }
        if let Some((client, variant)) = classify_process(&p.comm) {
            out.entry(client.to_string())
                .or_insert_with(|| variant.to_string());
            // Prefer cli variant over desktop when both present.
            if variant == "cli" {
                out.insert(client.to_string(), "cli".into());
            }
        }
    }
    out
}

fn parse_ps_line(line: &str) -> Option<ProcInfo> {
    let line = line.trim_start();
    let mut parts = line.splitn(3, |c: char| c.is_whitespace());
    // splitn may yield empty segments on repeated spaces; fall back to whitespace tokens for ids.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let pid: i64 = tokens[0].parse().ok()?;
    let ppid: i64 = tokens[1].parse().ok()?;
    let _ = &mut parts;
    // comm is remainder after second token in original line.
    let mut idx = 0usize;
    let bytes = line.as_bytes();
    let mut seen = 0;
    while idx < bytes.len() && seen < 2 {
        while idx < bytes.len() && !(bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
        seen += 1;
        while idx < bytes.len() && (bytes[idx] as char).is_whitespace() {
            idx += 1;
        }
    }
    Some(ProcInfo {
        pid,
        ppid,
        comm: line[idx..].trim().to_string(),
    })
}

pub fn list_processes() -> Vec<ProcInfo> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output();
    let Ok(output) = output else { return vec![] };
    if !output.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_ps_line)
        .collect()
}

// ---------- Store 持久化 ----------

impl Store {
    pub fn usage_insert_calls(&self, calls: &[UsageCall]) -> StoreResult<ImportResult> {
        let mut imported = 0i64;
        let mut skipped = 0i64;
        for c in calls {
            let n = self.conn.execute(
                "INSERT OR IGNORE INTO usage_calls(source,request_id,project,run_id,task_id,round,mode,provider,model,role,started_at,finished_at,status,budget_counted,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,reasoning_tokens,currency,actual_cost,estimated_cost,price_version,local_id,imported_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                params![c.source, c.request_id, c.project, c.run_id, c.task_id, c.round, c.mode, c.provider, c.model, c.role, c.started_at, c.finished_at, c.status, c.budget_counted as i64,
                    c.usage.as_ref().and_then(|u| u.input_tokens), c.usage.as_ref().and_then(|u| u.output_tokens),
                    c.usage.as_ref().and_then(|u| u.cache_read_tokens), c.usage.as_ref().and_then(|u| u.cache_creation_tokens),
                    c.usage.as_ref().and_then(|u| u.reasoning_tokens), c.currency, c.actual_cost, c.estimated_cost, c.price_version, c.local_id as i64, now_ms()],
            )?;
            if n == 0 {
                skipped += 1;
            } else {
                imported += 1;
            }
        }
        Ok(ImportResult {
            imported,
            skipped,
            invalid: 0,
        })
    }

    pub fn usage_sync_own(&self) -> StoreResult<()> {
        let value = self.export_api_calls()?;
        let raw = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        if let Ok(calls) = parse_import(&raw) {
            self.usage_update_own_calls(&calls)?;
        }
        Ok(())
    }

    pub fn usage_update_own_calls(&self, calls: &[UsageCall]) -> StoreResult<()> {
        for c in calls {
            let u = c.usage.as_ref();
            self.conn.execute(
                "UPDATE usage_calls SET status=?1,finished_at=?2,budget_counted=?3,input_tokens=?4,output_tokens=?5,cache_read_tokens=?6,cache_creation_tokens=?7,reasoning_tokens=?8 WHERE source=?9 AND request_id=?10",
                params![c.status, c.finished_at, c.budget_counted as i64,
                    u.and_then(|u| u.input_tokens), u.and_then(|u| u.output_tokens),
                    u.and_then(|u| u.cache_read_tokens), u.and_then(|u| u.cache_creation_tokens),
                    u.and_then(|u| u.reasoning_tokens), c.source, c.request_id],
            )?;
        }
        self.usage_insert_calls(calls)?;
        Ok(())
    }

    pub fn usage_calls(&self, filter: &UsageFilter) -> StoreResult<UsageListResult> {
        let mode = filter.mode.as_deref().unwrap_or("real");
        let mut stmt = self.conn.prepare("SELECT source,request_id,project,run_id,task_id,round,mode,provider,model,role,started_at,finished_at,status,budget_counted,input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,reasoning_tokens,currency,actual_cost,estimated_cost,price_version,local_id FROM usage_calls ORDER BY started_at DESC, rowid DESC")?;
        let rows = stmt
            .query_map([], |r| {
                let has_usage = r.get::<_, Option<i64>>(14)?.is_some()
                    || r.get::<_, Option<i64>>(15)?.is_some()
                    || r.get::<_, Option<i64>>(16)?.is_some()
                    || r.get::<_, Option<i64>>(17)?.is_some()
                    || r.get::<_, Option<i64>>(18)?.is_some()
                    || {
                        // usage object existed but all fields null is indistinguishable in columns;
                        // treat rows imported with usage null and known rows alike via started check below.
                        false
                    };
                Ok(UsageCall {
                    source: r.get(0)?,
                    request_id: r.get(1)?,
                    project: r.get(2)?,
                    run_id: r.get(3)?,
                    task_id: r.get(4)?,
                    round: r.get(5)?,
                    mode: r.get(6)?,
                    provider: r.get(7)?,
                    model: r.get(8)?,
                    role: r.get(9)?,
                    started_at: r.get(10)?,
                    finished_at: r.get(11)?,
                    status: r.get(12)?,
                    budget_counted: r.get::<_, i64>(13)? != 0,
                    usage: if has_usage {
                        Some(UsageTokens {
                            input_tokens: r.get(14)?,
                            output_tokens: r.get(15)?,
                            cache_read_tokens: r.get(16)?,
                            cache_creation_tokens: r.get(17)?,
                            reasoning_tokens: r.get(18)?,
                        })
                    } else {
                        None
                    },
                    currency: r.get(19)?,
                    actual_cost: r.get(20)?,
                    estimated_cost: r.get(21)?,
                    price_version: r.get(22)?,
                    local_id: r.get::<_, i64>(23)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // Rows whose usage was an object with all-null fields collapse to None; acceptable as unknown.
        let projects: Vec<String> = {
            let mut s: Vec<String> = rows.iter().map(|c| c.project.clone()).collect();
            s.sort();
            s.dedup();
            s.into_iter().filter(|p| !p.is_empty()).collect()
        };
        let models: Vec<String> = {
            let mut s: Vec<String> = rows.iter().map(|c| c.model.clone()).collect();
            s.sort();
            s.dedup();
            s
        };
        let providers: Vec<String> = {
            let mut s: Vec<String> = rows.iter().map(|c| c.provider.clone()).collect();
            s.sort();
            s.dedup();
            s.into_iter().filter(|p| !p.trim().is_empty()).collect()
        };
        let filtered: Vec<UsageCall> = rows
            .into_iter()
            .filter(|c| {
                (mode == "all" || c.mode == mode)
                    && filter.project.as_ref().is_none_or(|p| &c.project == p)
                    && filter.model.as_ref().is_none_or(|m| &c.model == m)
                    && filter.provider.as_ref().is_none_or(|p| &c.provider == p)
            })
            .collect();
        let summary = summarize(&filtered);
        Ok(UsageListResult {
            calls: filtered,
            summary,
            projects,
            models,
            providers,
        })
    }

    pub fn usage_save_snapshot(&self, card: &ClientCard) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO usage_snapshots(client,account_id,payload,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(client) DO UPDATE SET account_id=excluded.account_id,payload=excluded.payload,updated_at=excluded.updated_at",
            params![card.client, card.account_id, serde_json::to_string(card)?, now_ms()],
        )?;
        Ok(())
    }

    pub fn usage_snapshots(&self) -> StoreResult<HashMap<String, ClientCard>> {
        let mut stmt = self
            .conn
            .prepare("SELECT client,payload FROM usage_snapshots")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = HashMap::new();
        for (k, raw) in rows {
            if let Ok(card) = serde_json::from_str::<ClientCard>(&raw) {
                out.insert(k, card);
            }
        }
        Ok(out)
    }

    pub fn usage_save_balance(&self, card: &BalanceCard) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO usage_balances(provider,payload,updated_at) VALUES(?1,?2,?3) ON CONFLICT(provider) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",
            params![card.provider, serde_json::to_string(card)?, now_ms()],
        )?;
        Ok(())
    }

    pub fn usage_clear_balance(&self, provider: &str) -> StoreResult<()> {
        self.conn
            .execute("DELETE FROM usage_balances WHERE provider=?1", [provider])?;
        Ok(())
    }

    pub fn usage_balances(&self) -> StoreResult<Vec<BalanceCard>> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload FROM usage_balances ORDER BY provider")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect())
    }
}

// ── Gateway 的引擎报的额度（v0.5，只加写入口）──

/// 统一事件里的引擎 id → 额度卡片的客户端 id。
fn gateway_client(provider: &str) -> Option<&'static str> {
    match provider {
        "codex" => Some("codex"),
        "claude-code" => Some("claude-code"),
        "muse" => Some("muse-code"),
        "grok" => Some("grok"),
        "antigravity" => Some("antigravity"),
        _ => None,
    }
}

/// 引擎在任务里报的订阅窗口（统一事件 `quota.updated`）→ 这个客户端的额度卡片。账号沿用旧卡片的
/// （引擎没报账号；用非默认账号跑的不会走到这里，见 `EventSink::without_quota_sync`）。
pub fn gateway_quota_card(
    provider: &str,
    windows: &[crate::gateway::event::QuotaWindow],
    old: Option<&ClientCard>,
    collected_at: i64,
) -> Option<ClientCard> {
    let client = gateway_client(provider)?;
    if windows.is_empty() {
        return None;
    }
    let mut card = ClientCard::base(client);
    card.installed = true;
    card.running = true;
    card.collected_at = collected_at;
    card.account_id = old.and_then(|o| o.account_id.clone());
    card.account_label = old.and_then(|o| o.account_label.clone());
    card.tier = old.and_then(|o| o.tier.clone());
    card.windows = windows
        .iter()
        .map(|w| QuotaWindow {
            label: w.label.clone(),
            used_percent: w.used_percent,
            remaining_amount: None,
            reset_at_ms: w.resets_at,
            window_duration_mins: match w.label.as_str() {
                "5 小时" => Some(300),
                "每周" => Some(10080),
                "每天" => Some(1440),
                _ => None,
            },
        })
        .collect();
    card.status = "ok".into();
    card.fresh = true;
    card.last_success_at = Some(collected_at);
    card.source = "Peck 任务里引擎报的（Gateway）".into();
    card.raw_label = "Peck 跑任务时引擎随结果报的订阅窗口（统一事件 quota.updated）".into();
    Some(apply_success(old, card))
}

impl Store {
    pub fn usage_record_gateway_quota(
        &self,
        provider: &str,
        windows: &[crate::gateway::event::QuotaWindow],
    ) -> StoreResult<()> {
        let Some(client) = gateway_client(provider) else {
            return Ok(());
        };
        let old = self.usage_snapshots()?.remove(client);
        if let Some(card) = gateway_quota_card(provider, windows, old.as_ref(), now_ms()) {
            self.usage_save_snapshot(&card)?;
        }
        Ok(())
    }
}

// Account change: a successful card with a different non-null account replaces the old pool entirely.
pub fn apply_success(old: Option<&ClientCard>, mut new_card: ClientCard) -> ClientCard {
    if let Some(old) = old {
        if let (Some(old_id), Some(new_id)) = (&old.account_id, &new_card.account_id) {
            if old_id != new_id {
                new_card.last_failure_at = None;
                new_card.failure_reason = None;
                return new_card;
            }
        }
    }
    new_card
}

pub fn apply_failure(old: Option<&ClientCard>, mut failure: ClientCard) -> ClientCard {
    if let Some(old) = old {
        let same_account =
            matches!((&old.account_id, &failure.account_id), (Some(a), Some(b)) if a == b);
        if same_account && !old.windows.is_empty() {
            failure.windows = old.windows.clone();
            failure.last_success_at = old.last_success_at;
            failure.tier = old.tier.clone();
            if old.manual {
                failure.manual = true;
                failure.source = old.source.clone();
                failure.raw_label = old.raw_label.clone();
            }
        } else if !old.windows.is_empty() {
            failure.previous = Some(QuotaSnapshot {
                account_id: old.account_id.clone(),
                windows: old.windows.clone(),
                source: old.source.clone(),
                captured_at: old.last_success_at,
                manual: old.manual,
            });
        } else {
            failure.previous = old.previous.clone();
        }
    }
    failure.fresh = false;
    failure
}

// ---------- 安装与可执行文件定位 ----------

fn find_executable(command: &str, extra: &[String]) -> Option<PathBuf> {
    let expanded = crate::agent::expand_tilde(command);
    if expanded.components().count() > 1 || command.contains('/') {
        return if expanded.is_file() {
            Some(expanded)
        } else {
            None
        };
    }
    let path = crate::agent::build_augmented_path(extra);
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn manifest_command(id: &str) -> Option<(PathBuf, Vec<String>, HashMap<String, String>)> {
    let manifests = crate::agent::load_agent_manifests();
    let m = manifests.into_iter().find(|m| m.id == id)?;
    let resolved = find_executable(&m.command, &m.path)?;
    Some((resolved, m.path.clone(), m.env.clone()))
}

pub fn codex_binary() -> Option<PathBuf> {
    let bundled = PathBuf::from(
        "/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
    );
    if bundled.is_file() {
        return Some(bundled);
    }
    find_executable("codex", &[])
}

fn installed_map() -> HashMap<String, bool> {
    let mut map = HashMap::new();
    map.insert("muse-code".into(), manifest_command("muse").is_some());
    map.insert("grok".into(), manifest_command("grok").is_some());
    map.insert(
        "codex".into(),
        codex_binary().is_some() || PathBuf::from("/Applications/ChatGPT.app").exists(),
    );
    map.insert(
        "claude-code".into(),
        find_executable("claude", &[]).is_some(),
    );
    map.insert(
        "antigravity".into(),
        find_executable("antigravity", &[]).is_some()
            || PathBuf::from("/Applications/Antigravity.app").exists(),
    );
    map
}

// ---------- 子进程只读查询（超时杀进程组，不启动模型 turn） ----------

async fn wait_client_stopped(client: &str) {
    loop {
        if !running_clients(&list_processes(), std::process::id() as i64).contains_key(client) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn run_capture(
    program: &PathBuf,
    args: &[&str],
    env: &HashMap<String, String>,
    timeout_dur: Duration,
    stopped: impl std::future::Future<Output = ()>,
) -> Result<(bool, String, String), String> {
    run_capture_in(None, program, args, env, timeout_dur, stopped).await
}

async fn run_capture_in(
    cwd: Option<&std::path::Path>,
    program: &PathBuf,
    args: &[&str],
    env: &HashMap<String, String>,
    timeout_dur: Duration,
    stopped: impl std::future::Future<Output = ()>,
) -> Result<(bool, String, String), String> {
    let mut cmd = tokio::process::Command::new(program);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(args)
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 {}：{e}", program.display()))?;
    let pgid = child.id();
    let output = child.wait_with_output();
    tokio::pin!(output);
    let result = tokio::select! {
        out = &mut output => out.map(|out| (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())).map_err(|e| format!("查询进程失败：{e}")),
        _ = stopped => Err("用户客户端已退出，已取消额度查询。".into()),
        _ = tokio::time::sleep(timeout_dur) => Err(format!("查询超时（{} 秒）。", timeout_dur.as_secs())),
    };
    if result.is_err() {
        crate::process::terminate_process_group(pgid).await;
        let _ = tokio::time::timeout(Duration::from_secs(1), &mut output).await;
    }
    result
}

async fn query_muse() -> ClientCard {
    let now = now_ms();
    let Some((bin, _paths, env)) = manifest_command("muse") else {
        let mut c = ClientCard::base("muse-code");
        c.status = "not_installed".into();
        c.collected_at = now;
        return c;
    };
    let result = run_capture(
        &bin,
        &["usage", "--json"],
        &env,
        Duration::from_secs(10),
        wait_client_stopped("muse-code"),
    )
    .await;
    let now = now_ms();
    match result {
        Ok((true, stdout, _)) => match serde_json::from_str::<Value>(&stdout) {
            Ok(v) => parse_muse_usage(&v, now),
            Err(e) => {
                let mut c = ClientCard::base("muse-code");
                c.installed = true;
                c.running = true;
                c.collected_at = now;
                c.status = "read_failed".into();
                c.failure_reason = Some(format!("Muse usage 输出不是有效 JSON：{e}"));
                c.last_failure_at = Some(now);
                c
            }
        },
        Ok((false, stdout, stderr)) => {
            let msg = format!("{stderr}{stdout}")
                .trim()
                .chars()
                .take(300)
                .collect::<String>();
            let mut c = ClientCard::base("muse-code");
            c.installed = true;
            c.running = true;
            c.collected_at = now;
            c.status = if msg.to_lowercase().contains("login")
                || msg.contains("登录")
                || msg.contains("auth")
            {
                "not_logged_in".into()
            } else {
                "read_failed".into()
            };
            c.failure_reason = Some(if msg.is_empty() {
                "Muse usage --json 退出失败，未提供原因。".into()
            } else {
                msg
            });
            c.last_failure_at = Some(now);
            c
        }
        Err(e) => {
            let mut c = ClientCard::base("muse-code");
            c.installed = true;
            c.running = true;
            c.collected_at = now;
            c.status = "read_failed".into();
            c.failure_reason = Some(e);
            c.last_failure_at = Some(now);
            c
        }
    }
}

async fn query_grok() -> ClientCard {
    let now = now_ms();
    let Some((bin, _paths, env)) = manifest_command("grok") else {
        let mut c = ClientCard::base("grok");
        c.status = "not_installed".into();
        c.collected_at = now;
        return c;
    };
    match run_capture(
        &bin,
        &["--no-auto-update", "usage", "--json"],
        &env,
        Duration::from_secs(10),
        wait_client_stopped("grok"),
    )
    .await
    {
        Ok((true, stdout, _)) => match serde_json::from_str::<Value>(&stdout) {
            Ok(v) => {
                // Grok has no verified machine-readable plan fields yet; accept only explicit percent fields.
                let used = opt_f64(&v, "usedPercent").or_else(|| opt_f64(&v, "used_percent"));
                let mut c = ClientCard::base("grok");
                c.installed = true;
                c.running = true;
                c.collected_at = now;
                if let Some(u) = used {
                    c.windows = vec![QuotaWindow {
                        label: "订阅窗口".into(),
                        used_percent: Some(u),
                        remaining_amount: None,
                        reset_at_ms: opt_i64(&v, "resetsAtMs"),
                        window_duration_mins: None,
                    }];
                    c.status = "ok".into();
                    c.fresh = true;
                    c.last_success_at = Some(now);
                    c.raw_label = "Grok usage --json 的 usedPercent 字段".into();
                } else {
                    c.status = "fields_missing".into();
                    c.failure_reason = Some(
                        "Grok usage --json 已返回，但未包含可核实的已用比例字段；不填 0。".into(),
                    );
                    c.last_failure_at = Some(now);
                }
                c
            }
            Err(e) => {
                let mut c = ClientCard::base("grok");
                c.installed = true;
                c.running = true;
                c.collected_at = now;
                c.status = "read_failed".into();
                c.failure_reason = Some(format!("Grok usage 输出不是有效 JSON：{e}"));
                c.last_failure_at = Some(now);
                c
            }
        },
        Ok((false, stdout, stderr)) => {
            let msg = format!("{stderr}{stdout}")
                .trim()
                .chars()
                .take(400)
                .collect::<String>();
            let mut c = ClientCard::base("grok");
            c.installed = true;
            c.running = true;
            c.collected_at = now;
            // Known local limitation (tester-verified): not proof of not logged in.
            if msg.contains("can't show your plan") || msg.contains("Run /usage inside grok") {
                c.status = "fields_missing".into();
                c.failure_reason = Some("本机 Grok 1.0.x 的 usage --json 暂不支持套餐读取（提示 Run /usage inside grok instead）；这不等于未登录。可使用下方手工快照，或在 Grok 内执行 /usage 后录入。".into());
            } else if msg.to_lowercase().contains("login") || msg.contains("登录") {
                c.status = "not_logged_in".into();
                c.failure_reason = Some(msg.clone());
            } else {
                c.status = "read_failed".into();
                c.failure_reason = Some(if msg.is_empty() {
                    "Grok usage --json 退出失败，未提供原因。".into()
                } else {
                    msg
                });
            }
            c.last_failure_at = Some(now);
            c
        }
        Err(e) => {
            let mut c = ClientCard::base("grok");
            c.installed = true;
            c.running = true;
            c.collected_at = now;
            c.status = "read_failed".into();
            c.failure_reason = Some(e);
            c.last_failure_at = Some(now);
            c
        }
    }
}

async fn query_codex() -> ClientCard {
    let now = now_ms();
    let Some(bin) = codex_binary() else {
        let mut c = ClientCard::base("codex");
        c.status = "not_installed".into();
        c.collected_at = now;
        return c;
    };
    match codex_app_server(&bin).await {
        Ok((account, rate)) => parse_codex_rate_limits(&account, &rate, now),
        Err(e) => {
            let mut c = ClientCard::base("codex");
            c.installed = true;
            c.running = true;
            c.collected_at = now;
            c.status = if e.contains("未登录") || e.to_lowercase().contains("not logged") {
                "not_logged_in".into()
            } else {
                "read_failed".into()
            };
            c.failure_reason = Some(e);
            c.last_failure_at = Some(now);
            c
        }
    }
}

async fn codex_app_server(bin: &PathBuf) -> Result<(Value, Value), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["app-server"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动 Codex app-server：{e}"))?;
    let pgid = child.id();
    let request = async {
        let mut stdin = child.stdin.take().ok_or("app-server stdin 不可用")?;
        let stdout = child.stdout.take().ok_or("app-server stdout 不可用")?;
        let mut lines = BufReader::new(stdout).lines();
        async fn send(stdin: &mut tokio::process::ChildStdin, text: &str) -> Result<(), String> {
            stdin
                .write_all(text.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            stdin.write_all(b"\n").await.map_err(|e| e.to_string())?;
            stdin.flush().await.map_err(|e| e.to_string())
        }
        send(&mut stdin, r#"{"id":1,"method":"initialize","params":{"clientInfo":{"name":"peck-usage","title":"Peck Usage Desk","version":"0.2.0"},"capabilities":{"experimentalApi":true}}}"#).await?;
        let mut account: Option<Value> = None;
        let mut rate: Option<Value> = None;
        let mut initialized_sent = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while account.is_none() || rate.is_none() {
            let Some(line) = tokio::time::timeout_at(deadline, lines.next_line())
                .await
                .map_err(|_| "Codex app-server 读取超时（10 秒）。".to_string())?
                .map_err(|e| e.to_string())?
            else {
                break;
            };
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v.get("id").and_then(Value::as_i64) {
                Some(1) => {
                    if v.get("error").is_some() {
                        return Err(format!("Codex app-server initialize 失败：{}", v["error"]));
                    }
                    if !initialized_sent {
                        send(&mut stdin, r#"{"method":"initialized"}"#).await?;
                        send(
                            &mut stdin,
                            r#"{"id":2,"method":"account/read","params":{"refreshToken":false}}"#,
                        )
                        .await?;
                        send(
                            &mut stdin,
                            r#"{"id":3,"method":"account/rateLimits/read","params":null}"#,
                        )
                        .await?;
                        initialized_sent = true;
                    }
                }
                Some(2) => {
                    if let Some(e) = v.get("error") {
                        return Err(format!("Codex account/read 失败：{e}"));
                    }
                    account = Some(v.get("result").cloned().unwrap_or(Value::Null));
                }
                Some(3) => {
                    if let Some(e) = v.get("error") {
                        return Err(format!("Codex account/rateLimits/read 失败：{e}"));
                    }
                    rate = Some(v.get("result").cloned().unwrap_or(Value::Null));
                }
                _ => {}
            }
        }
        match (account, rate) {
            (Some(a), Some(r)) => {
                // Unwrap account/read's account object when present for type/accountId.
                let inner = a.get("account").cloned().unwrap_or(a);
                Ok((inner, r))
            }
            _ => Err("Codex app-server 未返回完整的 account/rateLimits 结果。".into()),
        }
    };
    let result = tokio::select! {
        result = request => result,
        _ = wait_client_stopped("codex") => Err("用户 Codex 客户端已退出，已取消额度查询。".into()),
    };
    crate::process::terminate_process_group(pgid).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

/// 读 Claude 额度用的 Claude Code：在哪、PATH、Peck 里选的账号目录、账号的名字。
pub struct ClaudeSetup {
    bin: PathBuf,
    path: String,
    env: Vec<(String, String)>,
    account: String,
}

async fn claude_setup(store_arc: &std::sync::Arc<Mutex<Store>>) -> Option<ClaudeSetup> {
    let (config, book) = {
        let store = store_arc.lock().ok()?;
        (
            crate::gateway::config::GatewayConfig::load(&store).ok()?,
            crate::gateway::accounts::AccountBook::load(&store),
        )
    };
    let path = config.search_path();
    let bin = crate::agent::resolve_command_path("claude", &path).await?;
    Some(ClaudeSetup {
        env: crate::gateway::accounts::env_for(&book, &config, "claude-code"),
        account: book.current("claude-code"),
        bin,
        path,
    })
}

/// Claude 的额度：跑 Claude Code 自带的 `/usage`（`claude -p /usage`）。它是本地命令，不进模型、不花额度，
/// 由 Claude Code 自己去问订阅用了多少；Peck 不读、不碰它的登录。时间用 UTC 打出来，好换算。
/// 读不到再看 status line 送来的（只有在终端里跑 Claude Code 时才有）。
async fn query_claude(setup: Option<&ClaudeSetup>, variant: &str) -> ClientCard {
    let now = now_ms();
    let Some(setup) = setup else {
        let mut c = event_driven_card("claude-code", variant, now);
        if c.status != "ok" {
            c.failure_reason = Some("没找到 Claude Code 命令行：在设置 → CLI 检测里填 CLI 目录（cli/bin），Peck 用它的 /usage 读额度。".into());
        }
        return c;
    };
    let mut env: HashMap<String, String> = setup.env.iter().cloned().collect();
    env.insert("PATH".into(), setup.path.clone());
    env.insert("TZ".into(), "UTC".into());
    let cwd = crate::mode::app_data_dir().join("cli-probe");
    let _ = std::fs::create_dir_all(&cwd);
    let account = Some(format!("peck:{}", setup.account));
    let out = run_capture_in(
        Some(&cwd),
        &setup.bin,
        &["-p", "/usage", "--output-format", "json", "--no-session-persistence"],
        &env,
        Duration::from_secs(25),
        wait_client_stopped("claude-code"),
    )
    .await;
    let mut card = match out {
        Ok((_, stdout, _)) => match serde_json::from_str::<Value>(stdout.trim()) {
            Ok(v) => parse_claude_usage(&v, now),
            Err(_) => failed_claude(now, "Claude Code 的 /usage 没给出能读的结果。"),
        },
        Err(e) => failed_claude(now, &e),
    };
    if card.status != "ok" {
        // 终端里跑的 Claude Code 可能经 status line 送来过。
        let event = event_driven_card("claude-code", "cli", now);
        if event.status == "ok" {
            return event;
        }
        if card.failure_reason.is_none() {
            card.failure_reason = Some(claude_login_hint(setup, &env, &cwd).await);
        }
    }
    card.account_id = account;
    card.account_label = (setup.account != "default").then(|| setup.account.clone());
    card.running = true;
    card.installed = true;
    card
}

fn failed_claude(now: i64, reason: &str) -> ClientCard {
    let mut c = ClientCard::base("claude-code");
    c.status = "query_failed".into();
    c.collected_at = now;
    c.last_failure_at = Some(now);
    c.failure_reason = Some(reason.into());
    c
}

/// /usage 里没有额度：多半是 Claude Code 命令行没按订阅登录（桌面 App 的登录和它不共用）。
async fn claude_login_hint(setup: &ClaudeSetup, env: &HashMap<String, String>, cwd: &std::path::Path) -> String {
    let status = run_capture_in(Some(cwd), &setup.bin, &["auth", "status", "--json"], env, Duration::from_secs(10), std::future::pending::<()>()).await;
    let logged_in = status
        .ok()
        .and_then(|(_, out, _)| serde_json::from_str::<Value>(out.trim()).ok())
        .and_then(|v| v.get("loggedIn").and_then(Value::as_bool));
    match logged_in {
        Some(false) => "Claude Code 命令行还没登录（桌面 App 的登录和它不共用）：在设置 → CLI 检测 → Claude Code 的账号点 LOGIN（或终端跑 cli/bin/claude auth login），登录和桌面 App 同一个订阅，Peck 就能读到额度。".into(),
        _ => "Claude Code 的 /usage 没给出订阅额度：可能登录的不是订阅（是 API Key），或者这个账号看不到额度。".into(),
    }
}

/// 解析 `claude -p /usage --output-format json` 的 `result`：`Current session: 23% used · resets Nov 14, 3pm (UTC)` 这样一行一个窗口。
pub fn parse_claude_usage(v: &Value, collected_at: i64) -> ClientCard {
    let mut c = ClientCard::base("claude-code");
    c.installed = true;
    c.running = true;
    c.collected_at = collected_at;
    let text = v.get("result").and_then(Value::as_str).unwrap_or("");
    let mut windows = vec![];
    for line in text.lines() {
        let line = line.trim();
        let Some((title, rest)) = line.split_once(": ") else { continue };
        if !title.starts_with("Current ") {
            continue;
        }
        let Some(Ok(pct)) = rest.split_once('%').map(|(p, _)| p.trim().parse::<f64>()) else { continue };
        let reset = rest.split_once("resets ").and_then(|(_, r)| parse_claude_reset(r, collected_at));
        let (label, mins) = claude_window(title);
        windows.push(QuotaWindow { label, used_percent: Some(pct), remaining_amount: None, reset_at_ms: reset, window_duration_mins: mins });
    }
    if windows.is_empty() {
        c.status = "fields_missing".into();
        c.last_failure_at = Some(collected_at);
    } else {
        c.status = "ok".into();
        c.windows = windows;
        c.last_success_at = Some(collected_at);
        c.fresh = true;
        if text.contains("using your overages") {
            c.tier = Some("在用超额".into());
        }
    }
    c
}

fn claude_window(title: &str) -> (String, Option<i64>) {
    match title {
        "Current session" => ("5 小时".into(), Some(300)),
        "Current week (all models)" => ("每周".into(), Some(10080)),
        t => match t.strip_prefix("Current week (").and_then(|x| x.strip_suffix(')')) {
            // 按模型分的周窗口：名字带上模型，不按时长叫（不然和「每周」重名）。
            Some(scope) => (format!("每周 {}", scope.trim_end_matches(" only")), None),
            None => (t.trim_start_matches("Current ").to_string(), None),
        },
    }
}

/// `Nov 14, 3pm (UTC)`、`Nov 14, 3:30pm (UTC)`、`Jan 2, 2027, 9am (UTC)`、`1:30pm (UTC)` → 毫秒。没写年就是今年（已经过了一天以上算明年）；只有时刻是今天（过了算明天）。
fn parse_claude_reset(s: &str, now_ms: i64) -> Option<i64> {
    let s = s.split(" (").next()?.trim();
    let parts: Vec<&str> = s.split(", ").collect();
    let (date, year, clock) = match parts.as_slice() {
        [c] => (None, None, *c),
        [d, c] => (Some(*d), None, *c),
        [d, y, c] => (Some(*d), y.trim().parse::<i32>().ok(), *c),
        _ => return None,
    };
    let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000).ok()?;
    let (month, day) = match date {
        Some(date) => {
            let (mon, day) = date.split_once(' ')?;
            let month = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
                .iter()
                .position(|m| mon.to_lowercase().starts_with(m))? as u8
                + 1;
            (month, day.trim().parse::<u8>().ok()?)
        }
        None => (u8::from(now.month()), now.day()),
    };
    let clock = clock.trim().to_lowercase();
    let pm = clock.ends_with("pm");
    let hm = clock.trim_end_matches("am").trim_end_matches("pm");
    let (h, m) = match hm.split_once(':') {
        Some((h, m)) => (h.parse::<u8>().ok()?, m.parse::<u8>().ok()?),
        None => (hm.parse::<u8>().ok()?, 0),
    };
    let hour = (h % 12) + if pm { 12 } else { 0 };
    let build = |y: i32| -> Option<i64> {
        let d = time::Date::from_calendar_date(y, time::Month::try_from(month).ok()?, day).ok()?;
        let t = time::Time::from_hms(hour, m, 0).ok()?;
        Some(time::PrimitiveDateTime::new(d, t).assume_utc().unix_timestamp() * 1000)
    };
    match (year, date) {
        (Some(y), _) => build(y),
        // 只有时刻：今天的这个时刻，过了就是明天。
        (None, None) => build(now.year()).map(|t| if t < now_ms { t + 86_400_000 } else { t }),
        (None, Some(_)) => {
            let this = build(now.year())?;
            if this < now_ms - 86_400_000 { build(now.year() + 1) } else { Some(this) }
        }
    }
}

fn event_driven_card(client: &str, variant: &str, collected_at: i64) -> ClientCard {
    let mut c = ClientCard::base(client);
    c.installed = true;
    c.running = true;
    c.collected_at = collected_at;
    c.status = "fields_missing".into();
    c.last_failure_at = Some(collected_at);
    if variant != "desktop" {
        let path = crate::mode::app_data_dir()
            .join("usage-events")
            .join(format!("{client}.json"));
        let event = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
        if let Some(event) = event {
            let pid = opt_i64(&event, "sourcePid").unwrap_or(0);
            let captured = opt_i64(&event, "capturedAt").unwrap_or(0);
            let procs = list_processes();
            let mut lineage = vec![];
            let mut cursor = pid;
            while let Some(p) = procs.iter().find(|p| p.pid == cursor) {
                lineage.push(p.clone());
                if p.ppid == cursor {
                    break;
                }
                cursor = p.ppid;
            }
            if event.get("client").and_then(Value::as_str) == Some(client)
                && (0..120_000).contains(&(collected_at - captured))
                && running_clients(&lineage, std::process::id() as i64).contains_key(client)
            {
                if let Some(fields) = event.get("fields") {
                    let mut card = if client == "claude-code" {
                        parse_claude_statusline(fields, captured)
                    } else {
                        parse_antigravity_quota(fields, captured)
                    };
                    card.account_id = opt_str(fields, "accountId");
                    card.collected_at = collected_at;
                    return card;
                }
            }
        }
    }
    c.failure_reason = Some(match (client, variant) {
        ("claude-code", _) => "还没读到 Claude 的额度。".into(),
        ("antigravity", "desktop") => "仅检测到 Antigravity 桌面 App；官方可核实的是 CLI statusline quota，桌面不等于 CLI 读取。Peck 未收到 CLI quota 事件。可手工录入快照。".into(),
        _ => "Antigravity 正在运行，但 Peck 尚未收到 CLI statusline 的 quota 事件；上下文比例不是订阅额度。可手工录入快照。".into(),
    });
    c
}

async fn query_client(client: &str, variant: &str, claude: Option<&ClaudeSetup>) -> ClientCard {
    match client {
        "muse-code" => query_muse().await,
        "grok" => query_grok().await,
        "codex" => query_codex().await,
        "claude-code" => query_claude(claude, variant).await,
        "antigravity" => event_driven_card("antigravity", variant, now_ms()),
        _ => ClientCard::base(client),
    }
}

pub fn stopped_card(
    old: Option<&ClientCard>,
    client: &str,
    installed: bool,
    collected_at: i64,
) -> ClientCard {
    match old {
        Some(old)
            if !old.windows.is_empty()
                || old.last_success_at.is_some()
                || old.previous.is_some() =>
        {
            let mut c = old.clone();
            c.status = "stale_stopped".into();
            c.fresh = false;
            c.running = false;
            c.installed = installed;
            c.collected_at = collected_at;
            c.manual = old.manual;
            c
        }
        _ => {
            let mut c = ClientCard::base(client);
            c.status = if installed {
                "not_running".into()
            } else {
                "not_installed".into()
            };
            c.installed = installed;
            c.collected_at = collected_at;
            c
        }
    }
}

// ---------- App 级监控（单例，关页不灭） ----------

pub struct UsageMonitor {
    pub last_query: Mutex<HashMap<String, i64>>,
    pub querying: Mutex<HashSet<String>>,
}

impl UsageMonitor {
    pub fn new() -> Self {
        Self {
            last_query: Mutex::new(HashMap::new()),
            querying: Mutex::new(HashSet::new()),
        }
    }
}

pub async fn refresh_clients(state: &AppState, force: bool) -> Vec<ClientCard> {
    refresh_clients_inner(state.store.clone(), state.usage_monitor.clone(), force).await
}

pub async fn refresh_clients_inner(
    store_arc: std::sync::Arc<Mutex<Store>>,
    monitor: std::sync::Arc<UsageMonitor>,
    _force: bool,
) -> Vec<ClientCard> {
    let now = now_ms();
    let (old_map, installed) = {
        let store = match store_arc.lock() {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        (store.usage_snapshots().unwrap_or_default(), installed_map())
    };
    let running = running_clients(&list_processes(), std::process::id() as i64);
    // Claude Code 按「CLI 检测」的目录找（`cli/bin`），带上 Peck 里选的账号目录。
    let claude = claude_setup(&store_arc).await;
    let mut cards = vec![];
    for client in CLIENTS {
        let old = old_map.get(client);
        let is_installed = installed.get(client).copied().unwrap_or(false)
            || (client == "claude-code" && (claude.is_some() || PathBuf::from("/Applications/Claude.app").exists()));
        if let Some(old) = old {
            if old.manual && old.status == "ok" && !running.contains_key(client) {
                // Manual snapshots persist until replaced; still mark not fresh.
                let mut c = old.clone();
                c.running = false;
                c.installed = is_installed;
                cards.push(c);
                continue;
            }
        }
        if !is_installed {
            let mut c = stopped_card(old, client, false, now);
            if old.is_none() || c.windows.is_empty() {
                c.status = "not_installed".into();
            }
            c.installed = false;
            c.running = false;
            persist_store(&store_arc, &c);
            cards.push(c);
            continue;
        }
        let Some(variant) = running.get(client) else {
            let c = stopped_card(old, client, true, now);
            persist_store(&store_arc, &c);
            cards.push(c);
            continue;
        };
        // Running: query at most every 60s per source, single-flight.
        let due = {
            let last = monitor
                .last_query
                .lock()
                .map(|m| m.get(client).copied())
                .unwrap_or(None);
            // Claude 的 /usage 每次要起一个 Claude Code：三分钟一次就够。
            let every = if client == "claude-code" { 180_000 } else { 60_000 };
            last.is_none_or(|t| now - t >= every) || old.is_none()
        };
        if !due {
            let mut c = old.cloned().unwrap_or_else(|| ClientCard::base(client));
            c.running = true;
            c.installed = true;
            c.fresh = c.status == "ok"
                && !c.manual
                && c.last_success_at
                    .is_some_and(|t| (0..90_000).contains(&(now - t)));
            cards.push(c);
            continue;
        }
        let already = {
            let mut q = match monitor.querying.lock() {
                Ok(q) => q,
                Err(_) => continue,
            };
            if q.contains(client) {
                true
            } else {
                q.insert(client.to_string());
                false
            }
        };
        if already {
            let mut c = old.cloned().unwrap_or_else(|| ClientCard::base(client));
            c.running = true;
            cards.push(c);
            continue;
        }
        let queried = query_client(client, variant, claude.as_ref()).await;
        if let Ok(mut m) = monitor.last_query.lock() {
            m.insert(client.to_string(), now_ms());
        }
        if let Ok(mut q) = monitor.querying.lock() {
            q.remove(client);
        }
        // A query finishing after the last user client exited must not refresh its snapshot.
        if !running_clients(&list_processes(), std::process::id() as i64).contains_key(client) {
            let card = stopped_card(old, client, is_installed, now_ms());
            persist_store(&store_arc, &card);
            cards.push(card);
            continue;
        }
        let card = if queried.status == "ok" {
            apply_success(old, queried)
        } else {
            apply_failure(old, queried)
        };
        persist_store(&store_arc, &card);
        cards.push(card);
    }
    cards
}

fn persist_store(store_arc: &std::sync::Arc<Mutex<Store>>, card: &ClientCard) {
    if let Ok(store) = store_arc.lock() {
        let _ = store.usage_save_snapshot(card);
    }
}

pub fn start_monitor(
    app: tauri::AppHandle,
    store_arc: std::sync::Arc<Mutex<Store>>,
    monitor: std::sync::Arc<UsageMonitor>,
    api: std::sync::Arc<crate::api::ApiState>,
) {
    tauri::async_runtime::spawn(async move {
        let mut last_tick = std::time::Instant::now();
        let mut last_wall = now_ms();
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let elapsed = last_tick.elapsed();
            let wall = now_ms();
            // Sleep/wake: wall clock jumps far beyond monotonic elapsed -> re-evaluate from scratch.
            let woke = (wall - last_wall) - elapsed.as_millis() as i64 > 30_000;
            if woke {
                if let Ok(mut m) = monitor.last_query.lock() {
                    m.clear();
                }
            }
            last_tick = std::time::Instant::now();
            last_wall = wall;
            let cards = refresh_clients_inner(store_arc.clone(), monitor.clone(), woke).await;
            let _ = app.emit("usage-updated", &cards);
            let active_deepseek = store_arc
                .lock()
                .ok()
                .and_then(|s| s.export_api_calls().ok())
                .is_some_and(|v| {
                    v.get("calls")
                        .and_then(Value::as_array)
                        .is_some_and(|calls| {
                            calls.iter().any(|c| {
                                c.get("provider").and_then(Value::as_str) == Some("deepseek")
                                    && c.get("mode").and_then(Value::as_str) == Some("real")
                                    && c.get("finishedAt").is_some_and(Value::is_null)
                            })
                        })
                });
            let cached_key = api
                .keys
                .lock()
                .ok()
                .is_some_and(|keys| keys.cached("deepseek").is_some());
            if active_deepseek && cached_key {
                let _ = refresh_balance(
                    "deepseek".into(),
                    api.clone(),
                    store_arc.clone(),
                    monitor.clone(),
                    false,
                )
                .await;
                if let Ok(store) = store_arc.lock() {
                    let _ = app.emit(
                        "usage-balances-updated",
                        store.usage_balances().unwrap_or_default(),
                    );
                }
            }
        }
    });
}

// ---------- Tauri commands ----------

#[tauri::command]
pub async fn usage_list(
    filter: UsageFilter,
    state: State<'_, AppState>,
) -> Result<UsageListResult, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.usage_sync_own().map_err(|e| e.to_string())?;
    store.usage_calls(&filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn usage_summary(
    filter: UsageFilter,
    state: State<'_, AppState>,
) -> Result<UsageSummary, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.usage_sync_own().map_err(|e| e.to_string())?;
    Ok(store
        .usage_calls(&filter)
        .map_err(|e| e.to_string())?
        .summary)
}

#[tauri::command]
pub async fn usage_import(
    json: String,
    state: State<'_, AppState>,
) -> Result<ImportResult, String> {
    let invalid = count_invalid(&json);
    let calls = parse_import(&json)?;
    let store = state.store.lock().map_err(|e| e.to_string())?;
    let mut result = store
        .usage_insert_calls(&calls)
        .map_err(|e| e.to_string())?;
    result.invalid = invalid;
    Ok(result)
}

#[tauri::command]
pub async fn usage_export_csv(
    window: tauri::WebviewWindow,
    filter: UsageFilter,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let csv = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.usage_sync_own().map_err(|e| e.to_string())?;
        build_csv(&store.usage_calls(&filter).map_err(|e| e.to_string())?.calls)
    };
    crate::export::save_export(
        &window,
        "导出用量台账 CSV",
        &format!("peck-usage-{}.csv", now_ms()),
        "CSV",
        "csv",
        csv.into_bytes(),
        "保存 CSV 失败",
    )
    .await
}

#[tauri::command]
pub async fn usage_clients(state: State<'_, AppState>) -> Result<Vec<ClientCard>, String> {
    Ok(refresh_clients(&state, false).await)
}

#[tauri::command]
pub async fn usage_refresh(state: State<'_, AppState>) -> Result<Vec<ClientCard>, String> {
    Ok(refresh_clients(&state, true).await)
}

#[tauri::command]
pub async fn usage_manual_snapshot(
    input: ManualSnapshotInput,
    state: State<'_, AppState>,
) -> Result<ClientCard, String> {
    if !CLIENTS.contains(&input.client.as_str()) {
        return Err("未知客户端。".into());
    }
    if input.note.trim().is_empty() {
        return Err("手工快照必须填写来源说明。".into());
    }
    if input.used_percent.is_none() {
        return Err("手工快照必须填写已用比例；未知请不要录入 0 代替。".into());
    }
    if let Some(p) = input.used_percent {
        if !(0.0..=100.0).contains(&p) {
            return Err("已用比例须在 0–100 之间。".into());
        }
    }
    let now = now_ms();
    let mut card = ClientCard::base(&input.client);
    card.status = "ok".into();
    card.manual = true;
    card.fresh = false;
    card.installed = installed_map()
        .get(input.client.as_str())
        .copied()
        .unwrap_or(false);
    card.running =
        running_clients(&list_processes(), std::process::id() as i64).contains_key(&input.client);
    card.collected_at = now;
    card.last_success_at = Some(now);
    card.source = format!("手工快照 · {}", input.note.trim());
    card.raw_label =
        "用户手工录入的已用比例（0–100），带来源说明与录入时间；与自动读取视觉区分".into();
    card.windows = vec![QuotaWindow {
        label: "手工快照窗口".into(),
        used_percent: input.used_percent,
        remaining_amount: None,
        reset_at_ms: input.reset_at_ms,
        window_duration_mins: None,
    }];
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store
        .usage_save_snapshot(&card)
        .map_err(|e| e.to_string())?;
    Ok(card)
}

#[tauri::command]
pub async fn usage_balance(
    provider: String,
    state: State<'_, AppState>,
) -> Result<BalanceCard, String> {
    refresh_balance(
        provider,
        state.api.clone(),
        state.store.clone(),
        state.usage_monitor.clone(),
        true,
    )
    .await
}

async fn refresh_balance(
    provider: String,
    api: std::sync::Arc<crate::api::ApiState>,
    store_arc: std::sync::Arc<Mutex<Store>>,
    monitor: std::sync::Arc<UsageMonitor>,
    load_key: bool,
) -> Result<BalanceCard, String> {
    let now = now_ms();
    if provider == "anthropic" {
        let card = BalanceCard {
            provider: "anthropic".into(), label: "Anthropic API".into(), status: "unknown".into(),
            balances: vec![], checked_at: Some(now),
            message: "未知：普通推理 Key 无官方余额接口；Usage/Cost Analytics 需组织 Admin 权限且不等于余额，未调用冒充。".into(),
            last_success_at: None, last_failure_at: None,
        };
        if let Ok(store) = store_arc.lock() {
            let _ = store.usage_save_balance(&card);
        }
        return Ok(card);
    }
    if provider != "deepseek" {
        return Err("未知供应商。".into());
    }
    let old = store_arc
        .lock()
        .map_err(|_| "无法读取余额快照。")?
        .usage_balances()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|b| b.provider == provider);
    let query_id = format!("balance/{provider}");
    {
        let mut queries = monitor.querying.lock().map_err(|_| "余额状态忙。")?;
        let recent = monitor
            .last_query
            .lock()
            .map_err(|_| "余额状态忙。")?
            .get(&query_id)
            .is_some_and(|t| now - t < 60_000);
        if queries.contains(&query_id) || recent {
            return old.ok_or_else(|| "余额正在查询，请稍后查看。".into());
        }
        queries.insert(query_id.clone());
    }
    let mut revision = None;
    let result: Result<BalanceCard, String> = async {
        // Automatic polling uses only the already-loaded key: never opens a Keychain prompt.
        let api_for_key = api.clone();
        let key_result = if load_key {
            tokio::task::spawn_blocking(move || {
                let mut guard = api_for_key
                    .keys
                    .lock()
                    .map_err(|_| "无法读取密钥。".to_string())?;
                let key = guard.get("deepseek")?;
                Ok((key, guard.revision()))
            })
            .await
            .map_err(|_| "读取密钥任务中断。".to_string())
            .and_then(|r| r)
        } else {
            let keys = api.keys.lock().map_err(|_| "密钥状态忙。".to_string())?;
            keys.cached("deepseek")
                .map(|key| (key, keys.revision()))
                .ok_or_else(|| "未加载 DeepSeek Key。".to_string())
        };
        let key = match key_result {
            Ok((key, version)) => {
                revision = Some(version);
                key
            }
            Err(e) => {
                let card = BalanceCard {
                    provider: "deepseek".into(),
                    label: "DeepSeek API".into(),
                    status: "missing_key".into(),
                    balances: vec![],
                    checked_at: Some(now),
                    message: format!(
                        "未配置 DeepSeek Key：{e} 请在 API 工作台保存 DeepSeek Key 后再查询。"
                    ),
                    last_success_at: None,
                    last_failure_at: Some(now),
                };
                return Ok(card);
            }
        };
        let client = crate::api::provider::client().map_err(|e| e.to_string())?;
        let response = client
            .get("https://api.deepseek.com/user/balance")
            .header("Authorization", format!("Bearer {}", key.as_str()))
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        let card = match response {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("DeepSeek 余额响应无法解析：{e}"))?;
                let balances: Vec<BalanceInfo> = body
                    .get("balance_infos")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .map(|b| BalanceInfo {
                                currency: opt_str(b, "currency")
                                    .unwrap_or_else(|| "未知币种".into()),
                                total_balance: opt_str(b, "total_balance")
                                    .unwrap_or_else(|| "未知".into()),
                                granted_balance: opt_str(b, "granted_balance"),
                                topped_up_balance: opt_str(b, "topped_up_balance"),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                BalanceCard {
                    provider: "deepseek".into(),
                    label: "DeepSeek API".into(),
                    status: if balances.is_empty() {
                        "unknown".into()
                    } else {
                        "ok".into()
                    },
                    balances,
                    checked_at: Some(now),
                    message: if body.get("is_available").and_then(Value::as_bool) == Some(false) {
                        "DeepSeek 返回账户当前不可用（is_available=false），金额以官方返回为准。"
                            .into()
                    } else {
                        "官方 GET /user/balance 返回金额与币种，未用本地费用推算。".into()
                    },
                    last_success_at: Some(now),
                    last_failure_at: None,
                }
            }
            Ok(resp) => BalanceCard {
                provider: "deepseek".into(),
                label: "DeepSeek API".into(),
                status: "read_failed".into(),
                balances: vec![],
                checked_at: Some(now),
                message: format!(
                    "DeepSeek 余额查询失败（HTTP {}）；未用充值减本地费用推算。",
                    resp.status().as_u16()
                ),
                last_success_at: None,
                last_failure_at: Some(now),
            },
            Err(e) => BalanceCard {
                provider: "deepseek".into(),
                label: "DeepSeek API".into(),
                status: "read_failed".into(),
                balances: vec![],
                checked_at: Some(now),
                message: format!("DeepSeek 余额查询失败：{e}"),
                last_success_at: None,
                last_failure_at: Some(now),
            },
        };
        Ok(card)
    }
    .await;
    if let Ok(mut queries) = monitor.querying.lock() {
        queries.remove(&query_id);
    }
    if revision.is_some_and(|version| {
        api.keys
            .lock()
            .map(|keys| keys.revision() != version)
            .unwrap_or(true)
    }) {
        return Err("查询期间 Key 已变更，已丢弃旧账户结果。".into());
    }
    let mut card = result.unwrap_or_else(|error| BalanceCard {
        provider,
        label: "DeepSeek API".into(),
        status: "read_failed".into(),
        balances: vec![],
        checked_at: Some(now),
        message: error,
        last_success_at: None,
        last_failure_at: Some(now),
    });
    if card.status != "ok" {
        if let Some(old) = old.filter(|b| !b.balances.is_empty()) {
            card.balances = old.balances;
            card.last_success_at = old.last_success_at.or(old.checked_at);
            card.message
                .push_str(" 下方仅为账号未核验的上次成功快照，不代表本次查询或当前账户余额。");
        }
    }
    if let Ok(mut queries) = monitor.last_query.lock() {
        queries.insert(query_id, now_ms());
    }
    store_arc
        .lock()
        .map_err(|_| "无法保存余额快照。")?
        .usage_save_balance(&card)
        .map_err(|e| e.to_string())?;
    Ok(card)
}

#[tauri::command]
pub async fn usage_balances(state: State<'_, AppState>) -> Result<Vec<BalanceCard>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.usage_balances().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture_call(
        id: &str,
        mode: &str,
        usage: Option<UsageTokens>,
        currency: Option<&str>,
        actual: Option<f64>,
    ) -> UsageCall {
        UsageCall {
            source: "peck".into(),
            request_id: id.into(),
            project: "fixture-project".into(),
            run_id: None,
            task_id: None,
            round: Some(1),
            mode: mode.into(),
            provider: "fixture".into(),
            model: "fixture-model".into(),
            role: "worker".into(),
            started_at: 1000,
            finished_at: Some(2000),
            status: "completed".into(),
            budget_counted: true,
            usage,
            currency: currency.map(str::to_string),
            actual_cost: actual,
            estimated_cost: None,
            price_version: None,
            local_id: false,
        }
    }

    fn memory_store() -> Store {
        let path =
            std::env::temp_dir().join(format!("peck-usage-fixture-{}.db", uuid::Uuid::new_v4()));
        Store::open(&path).unwrap()
    }

    #[test]
    fn usage_import_dedupes_on_second_import_fixture() {
        let store = memory_store();
        let json = r#"{"schemaVersion":1,"source":"peck","coverage":"fixture","exportedAt":1,"calls":[{"source":"peck","requestId":"fixture-1","project":"p","mode":"real","provider":"x","model":"m","role":"worker","startedAt":1,"finishedAt":2,"status":"completed","budgetCounted":true,"usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":null,"cacheCreationTokens":null,"reasoningTokens":null},"currency":null,"actualCost":null,"estimatedCost":null,"priceVersion":null}]}"#;
        let calls = parse_import(json).unwrap();
        let first = store.usage_insert_calls(&calls).unwrap();
        let second = store.usage_insert_calls(&calls).unwrap();
        assert_eq!(first.imported, 1);
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 1);
        let list = store.usage_calls(&UsageFilter::default()).unwrap();
        assert_eq!(list.summary.request_count, 1);
    }

    #[test]
    fn usage_unknown_tokens_and_costs_are_not_zero_fixture() {
        let calls = vec![fixture_call("a", "real", None, Some("USD"), None)];
        let s = summarize(&calls);
        assert_eq!(s.tokens.input, None);
        assert_eq!(s.tokens.unknown_count, 1);
        assert_eq!(s.costs[0].actual, None);
        assert_eq!(s.costs[0].actual_unknown_count, 1);
    }

    #[test]
    fn usage_multi_currency_never_merges_fixture() {
        let calls = vec![
            fixture_call("a", "real", None, Some("USD"), Some(1.0)),
            fixture_call("b", "real", None, Some("CNY"), Some(7.0)),
        ];
        let s = summarize(&calls);
        assert_eq!(s.costs.len(), 2);
        assert!(s
            .costs
            .iter()
            .any(|c| c.currency.as_deref() == Some("USD") && c.actual == Some(1.0)));
        assert!(s
            .costs
            .iter()
            .any(|c| c.currency.as_deref() == Some("CNY") && c.actual == Some(7.0)));
    }

    #[test]
    fn usage_default_filter_is_real_only_fixture() {
        let store = memory_store();
        store
            .usage_insert_calls(&[
                fixture_call("r1", "real", None, None, None),
                fixture_call("m1", "mock", None, None, None),
            ])
            .unwrap();
        assert_eq!(
            store
                .usage_calls(&UsageFilter::default())
                .unwrap()
                .summary
                .request_count,
            1
        );
        let all = store
            .usage_calls(&UsageFilter {
                mode: Some("all".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all.summary.request_count, 2);
    }

    #[test]
    fn usage_provider_filter_isolates_real_and_mock_fixture() {
        let store = memory_store();
        let mut deepseek = fixture_call(
            "ds-real",
            "real",
            Some(UsageTokens {
                input_tokens: Some(100),
                output_tokens: Some(10),
                ..Default::default()
            }),
            Some("CNY"),
            Some(1.5),
        );
        deepseek.provider = "deepseek".into();
        deepseek.model = "deepseek-chat".into();
        let mut anthropic = fixture_call(
            "an-real",
            "real",
            Some(UsageTokens {
                input_tokens: Some(7),
                ..Default::default()
            }),
            Some("USD"),
            Some(0.25),
        );
        anthropic.provider = "anthropic".into();
        anthropic.model = "claude-sonnet-4-5".into();
        let mut mocked = fixture_call(
            "ds-mock",
            "mock",
            Some(UsageTokens {
                input_tokens: Some(5000),
                ..Default::default()
            }),
            None,
            None,
        );
        mocked.provider = "deepseek".into();
        mocked.model = "deepseek-chat".into();
        store
            .usage_insert_calls(&[deepseek, anthropic, mocked])
            .unwrap();

        // 默认 real：mock 的 deepseek 记录不得混入真实汇总。
        let real = store.usage_calls(&UsageFilter::default()).unwrap();
        assert_eq!(real.summary.request_count, 2);
        assert_eq!(real.summary.tokens.input, Some(107));
        assert_eq!(
            real.providers,
            vec!["anthropic".to_string(), "deepseek".to_string()]
        );

        // real + deepseek：只剩真实 DeepSeek，token 与费用不串到 Anthropic 或 mock。
        let ds = store
            .usage_calls(&UsageFilter {
                provider: Some("deepseek".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ds.calls.len(), 1);
        assert_eq!(ds.summary.request_count, 1);
        assert_eq!(ds.summary.tokens.input, Some(100));
        assert_eq!(ds.summary.tokens.output, Some(10));
        assert_eq!(ds.summary.costs.len(), 1);
        assert_eq!(ds.summary.costs[0].currency.as_deref(), Some("CNY"));
        assert_eq!(ds.summary.costs[0].actual, Some(1.5));
        assert_eq!(ds.calls[0].mode, "real");

        // 切到 mock 模式时同供应商只取 mock 记录，真实记录不重复计数。
        let mock_only = store
            .usage_calls(&UsageFilter {
                mode: Some("mock".into()),
                provider: Some("deepseek".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(mock_only.summary.request_count, 1);
        assert_eq!(mock_only.summary.tokens.input, Some(5000));

        // Anthropic 只带自己的 USD 费用。
        let an = store
            .usage_calls(&UsageFilter {
                provider: Some("anthropic".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(an.summary.request_count, 1);
        assert_eq!(an.summary.tokens.input, Some(7));
        assert_eq!(an.summary.costs.len(), 1);
        assert_eq!(an.summary.costs[0].currency.as_deref(), Some("USD"));

        // 未记录的供应商返回空列表与零汇总，不补默认数据。
        let none = store
            .usage_calls(&UsageFilter {
                provider: Some("openai".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(none.calls.is_empty());
        assert_eq!(none.summary.request_count, 0);
    }

    #[test]
    fn usage_muse_parser_uses_tester_schema_fixture() {
        let v = json!({"schemaVersion":1,"allowance":{"tier":"pro","window":{"usedPercent":25.0,"resetsAtMs":1700000000000i64},"weekly":{"usedPercent":10.0,"resetsAtMs":1700500000000i64},"asOfMs":1699990000000i64,"source":"mint"}});
        let card = parse_muse_usage(&v, 1700000000000);
        assert_eq!(card.status, "ok");
        assert_eq!(card.account_id, None);
        assert_eq!(card.windows.len(), 2);
        assert_eq!(card.windows[0].used_percent, Some(25.0));
        assert_eq!(card.last_success_at, Some(1699990000000));
        assert!(!card.fresh);
        assert!(card.raw_label.contains("mint"));
        let missing = parse_muse_usage(&json!({"schemaVersion":1,"allowance":{"tier":"pro"}}), 1);
        assert_eq!(missing.status, "fields_missing");
    }

    #[test]
    fn usage_import_rejects_missing_ids_and_updates_own_request() {
        let raw = r#"{"schemaVersion":1,"source":"peck","calls":[{"source":"peck","mode":"real","model":"fixture"}]}"#;
        assert!(parse_import(raw).unwrap().is_empty());
        assert_eq!(count_invalid(raw), 1);
        let store = memory_store();
        let mut call = fixture_call("same-request", "real", None, None, None);
        call.status = "running".into();
        call.finished_at = None;
        store.usage_update_own_calls(&[call.clone()]).unwrap();
        call.status = "completed".into();
        call.finished_at = Some(2000);
        call.usage = Some(UsageTokens {
            input_tokens: Some(123),
            ..Default::default()
        });
        store.usage_update_own_calls(&[call]).unwrap();
        let list = store.usage_calls(&UsageFilter::default()).unwrap();
        assert_eq!(list.calls.len(), 1);
        assert_eq!(list.calls[0].status, "completed");
        assert_eq!(list.summary.tokens.input, Some(123));
    }

    #[test]
    fn usage_failure_keeps_unverified_and_manual_history_separate() {
        let mut old = parse_muse_usage(
            &json!({"allowance":{"window":{"usedPercent":25},"asOfMs":100}}),
            100,
        );
        old.manual = true;
        old.source = "手工录入 fixture".into();
        let mut failure = ClientCard::base("muse-code");
        failure.status = "read_failed".into();
        failure.account_id = Some("new-account".into());
        let result = apply_failure(Some(&old), failure);
        assert!(result.windows.is_empty());
        assert!(!result.fresh);
        assert!(!result.manual);
        assert_eq!(result.account_id.as_deref(), Some("new-account"));
        let history = result.previous.as_ref().unwrap();
        assert!(history.manual);
        assert_eq!(history.captured_at, Some(100));
        assert_eq!(history.source, "手工录入 fixture");
        assert_eq!(history.windows[0].used_percent, Some(25.0));
        assert!(stopped_card(Some(&result), "muse-code", true, 300)
            .previous
            .is_some());
    }

    #[test]
    fn usage_codex_equal_windows_in_distinct_pools_are_retained() {
        let rate = json!({"accountId":"fixture-account","rateLimitsByLimitId":{
            "pool-a":{"primary":{"usedPercent":10,"resetsAt":1700000000i64,"windowDurationMins":300}},
            "pool-b":{"primary":{"usedPercent":10,"resetsAt":1700000000i64,"windowDurationMins":300}}
        }});
        let parsed = parse_codex_rate_limits(&json!({"type":"chatgpt"}), &rate, 1);
        assert_eq!(parsed.windows.len(), 2);
        assert_ne!(parsed.windows[0].label, parsed.windows[1].label);
    }

    #[test]
    fn usage_antigravity_parses_amount_and_timezone() {
        let data = json!({"quota":{"fixture":{"remaining_amount":42,"reset_time":"2026-07-06T15:50:32+08:00"}}});
        let parsed = parse_antigravity_quota(&data, 1);
        assert_eq!(parsed.status, "ok");
        assert_eq!(parsed.windows[0].used_percent, None);
        assert_eq!(parsed.windows[0].remaining_amount, Some(42.0));
        assert_eq!(
            parsed.windows[0].reset_at_ms,
            parse_reset_time("2026-07-06T07:50:32Z")
        );
        assert!(parsed.windows[0].reset_at_ms.is_some());
    }

    #[tokio::test]
    async fn usage_cancel_slow_query_terminates_process() {
        let dir = std::env::temp_dir().join(format!("peck-usage-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("pid");
        let command = format!("echo $$ > '{}'; exec sleep 20", pidfile.display());
        let stop = async {
            while !pidfile.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        let started = std::time::Instant::now();
        let result = run_capture(
            &PathBuf::from("/bin/sh"),
            &["-c", &command],
            &HashMap::new(),
            Duration::from_secs(10),
            stop,
        )
        .await;
        assert!(result.unwrap_err().contains("客户端已退出"));
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(&pidfile).unwrap();
        assert!(!std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn usage_codex_parser_does_not_double_count_compat_fixture() {
        let account = json!({"type":"chatgpt"});
        let rate = json!({"accountId":"acct-fixture","rateLimits":{"primary":{"usedPercent":99.0,"windowDurationMins":300,"resetsAt":1700000000i64}},"rateLimitsByLimitId":{"codex":{"limitName":"Codex","primary":{"usedPercent":40.0,"windowDurationMins":300,"resetsAt":1700000000i64},"secondary":{"usedPercent":20.0,"windowDurationMins":10080,"resetsAt":1700500000i64}}}});
        let card = parse_codex_rate_limits(&account, &rate, 1700000000000);
        assert_eq!(card.status, "ok");
        assert_eq!(card.account_id.as_deref(), Some("acct-fixture"));
        assert_eq!(card.windows.len(), 2);
        assert!(card.windows.iter().all(|w| w.used_percent != Some(99.0)));
    }

    #[test]
    fn usage_claude_and_antigravity_missing_fields_fixture() {
        assert_eq!(
            parse_claude_statusline(&json!({}), 1).status,
            "fields_missing"
        );
        let ok = parse_claude_statusline(
            &json!({"rate_limits":{"five_hour":{"used_percentage":30.0,"resets_at":1700000000i64}}}),
            1,
        );
        assert_eq!(ok.status, "ok");
        assert_eq!(
            parse_antigravity_quota(&json!({}), 1).status,
            "fields_missing"
        );
        let ag = parse_antigravity_quota(
            &json!({"quota":{"model-a":{"remaining_fraction":0.25,"reset_time":"1700000000"}}}),
            1,
        );
        assert_eq!(ag.windows[0].used_percent, Some(75.0));
    }

    #[test]
    fn usage_process_detection_excludes_peck_subtree_and_terminal_fixture() {
        let procs = vec![
            ProcInfo {
                pid: 100,
                ppid: 1,
                comm: "/Applications/Peck.app/Contents/MacOS/Peck".into(),
            },
            ProcInfo {
                pid: 101,
                ppid: 100,
                comm: "/usr/local/bin/claude".into(),
            },
            ProcInfo {
                pid: 102,
                ppid: 1,
                comm: "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal".into(),
            },
            ProcInfo {
                pid: 103,
                ppid: 1,
                comm: "/Users/test/.local/bin/claude".into(),
            },
            ProcInfo {
                pid: 104,
                ppid: 1,
                comm: "/opt/homebrew/bin/grok".into(),
            },
        ];
        let running = running_clients(&procs, 100);
        assert!(running.contains_key("claude-code"));
        assert!(running.contains_key("grok"));
        // pid 101 is a Peck child (query/dev subprocess) and must not be the sole evidence;
        // pid 103 is the independent user CLI that keeps claude-code running.
        let only_child = vec![
            ProcInfo {
                pid: 100,
                ppid: 1,
                comm: "/Applications/Peck.app/Contents/MacOS/Peck".into(),
            },
            ProcInfo {
                pid: 101,
                ppid: 100,
                comm: "/usr/local/bin/claude".into(),
            },
        ];
        assert!(running_clients(&only_child, 100).is_empty());
        assert_eq!(
            classify_process("/Users/fixture/.local/share/claude/versions/2.1.318"),
            Some(("claude-code", "cli"))
        );
        // 桌面 App 的主进程算在跑（和 Claude Code 共用订阅额度），辅助进程不算。
        assert_eq!(
            classify_process("/Applications/Claude.app/Contents/MacOS/Claude"),
            Some(("claude-code", "desktop"))
        );
        assert_eq!(
            classify_process("/Applications/Claude.app/Contents/Frameworks/Claude Helper (Renderer).app/Contents/MacOS/Claude Helper (Renderer)"),
            None
        );
    }

    #[test]
    fn claude_usage_lines_become_windows() {
        let now = 1_794_556_800_000; // 2026-11-13 08:00 UTC
        let v = json!({"type": "result", "local_command": "usage", "result": "You are currently using your subscription to power your Claude Code usage\n\nCurrent session: 23% used · resets 1:30pm (UTC)\nCurrent week (all models): 61% used · resets Nov 17, 2am (UTC)\nCurrent week (Sonnet only): 4% used · resets Nov 17, 2am (UTC)\nCurrent week (Opus): 70% used"});
        let c = parse_claude_usage(&v, now);
        assert_eq!(c.status, "ok");
        let w: Vec<_> = c.windows.iter().map(|w| (w.label.as_str(), w.used_percent, w.window_duration_mins)).collect();
        assert_eq!(w, vec![("5 小时", Some(23.0), Some(300)), ("每周", Some(61.0), Some(10080)), ("每周 Sonnet", Some(4.0), None), ("每周 Opus", Some(70.0), None)]);
        assert_eq!(c.windows[3].reset_at_ms, None);
        // 只有时刻的：今天的 13:30 UTC（过了就是明天）。
        assert_eq!(c.windows[0].reset_at_ms, Some(now + 5 * 3_600_000 + 30 * 60_000));
        assert_eq!(parse_claude_reset("Nov 17, 2am (UTC)", now), Some(1_794_880_800_000));
        assert_eq!(parse_claude_reset("Jan 2, 2027, 12pm (UTC)", now), Some(1_798_891_200_000));
        // 没登录订阅时只有花费那几行：读不到，不算 0。
        let none = parse_claude_usage(&json!({"result": "Total cost:            $0.0000\nTotal duration (API):  0s"}), now);
        assert_eq!(none.status, "fields_missing");
        assert!(none.windows.is_empty());
    }

    #[test]
    fn usage_snapshot_stop_and_account_change_fixture() {
        let mut ok = ClientCard::base("codex");
        ok.status = "ok".into();
        ok.account_id = Some("acct-a".into());
        ok.windows = vec![QuotaWindow {
            label: "w".into(),
            used_percent: Some(10.0),
            remaining_amount: None,
            reset_at_ms: None,
            window_duration_mins: None,
        }];
        ok.last_success_at = Some(100);
        let stopped = stopped_card(Some(&ok), "codex", true, 200);
        assert_eq!(stopped.status, "stale_stopped");
        assert_eq!(stopped.windows.len(), 1);
        let mut new_account = ok.clone();
        new_account.account_id = Some("acct-b".into());
        new_account.windows = vec![QuotaWindow {
            label: "w2".into(),
            used_percent: Some(5.0),
            remaining_amount: None,
            reset_at_ms: None,
            window_duration_mins: None,
        }];
        let applied = apply_success(Some(&ok), new_account);
        assert_eq!(applied.account_id.as_deref(), Some("acct-b"));
        assert_eq!(applied.windows[0].label, "w2");
        // Persistence round-trip in isolated fixture DB.
        let store = memory_store();
        store.usage_save_snapshot(&ok).unwrap();
        let loaded = store.usage_snapshots().unwrap();
        assert_eq!(loaded["codex"].account_id.as_deref(), Some("acct-a"));
    }

    #[test]
    fn usage_csv_has_header_first_and_only_record_rows() {
        let csv = build_csv(&[fixture_call("a", "real", None, Some("USD"), None)]);
        let lines: Vec<_> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("source,requestId,"));
        assert!(lines[1].starts_with("peck,a,"));
        assert_eq!(lines[0].split(',').count(), lines[1].split(',').count());
        assert_eq!(build_csv(&[]).lines().count(), 1);
        assert!(csv.contains(",USD,"));
    }
}
