//! API 价格表（v0.10 Refine，站主 2026-11-17 要的）：花费一律按「用量 × 价格表」算。
//!
//! - 内置一份 `defaults/prices.json`（联网查的：美元价取 models.dev，国内的取官方人民币价），站主在设置 → QUOTA 里能改、加、
//!   刷新（REFRESH 从 models.dev 取美元价；人民币的不跟着刷新，models.dev 只有折算后的美元价）。
//! - 国内的模型（DeepSeek、Aliyun）按人民币记，汇率默认 1 美元 = 6.7 人民币，能改；算出来的花费都折成美元（上限、QUOTA 都是美元）。
//! - 用在两处：Peck Loop 每次请求的花费（`ModelSpec::cost` 先查这里，查不到才用目录里的价格），QUOTA 页给 CLI 折算（`spend.rs`）。
//! - 站主改的、刷新来的存在 `app_settings` 的 `prices`；内置的随版本更新，改过的盖在上面。

use super::catalog::PriceTier;
use super::event::TokenUsage;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{OnceLock, RwLock};

const BUILTIN: &str = include_str!("../../defaults/prices.json");
const SETTING_KEY: &str = "prices";
const MODELS_DEV: &str = "https://models.dev/api.json";
pub const DEFAULT_FX: f64 = 6.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    Usd,
    Cny,
}

/// 一个模型的价格：每百万 tokens，币种见 `currency`。缓存读写没单价的按输入价算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRow {
    /// 模型名（发给供应商的那个，不带供应商；Vertex 的 `@default` 去掉）。CLI 报的模型名也按它找。
    pub model: String,
    pub label: String,
    /// 谁家的：anthropic、openai、google、deepseek、aliyun、xai、meta。
    pub vendor: String,
    pub currency: Currency,
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    /// 提示超过 `above_tokens` 以后整条请求按这一档算（CLI 报的是一个回合的合计，不分档）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<PriceTier>,
    /// 高峰时段的倍数（DeepSeek：北京时间工作日 9–12、14–18 点是 2 倍，其余时间是表里的价）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<f64>,
}

/// 高峰时段：北京时间（UTC+8）周一到周五 9:00–12:00、14:00–18:00（不管节假日）。
pub fn is_peak(at_ms: i64) -> bool {
    let local = at_ms + 8 * 3_600_000;
    let days = local.div_euclid(86_400_000);
    // 1970-01-01 是周四：周一 = 0。
    let weekday = (days + 3).rem_euclid(7);
    let hour = local.rem_euclid(86_400_000) / 3_600_000;
    weekday < 5 && ((9..12).contains(&hour) || (14..18).contains(&hour))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vendor {
    pub id: String,
    pub label: String,
    /// models.dev 里的供应商 id（刷新用）；人民币的为空。
    #[serde(default)]
    pub models_dev: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Builtin {
    date: String,
    vendors: Vec<Vendor>,
    rows: Vec<PriceRow>,
}

fn builtin() -> &'static Builtin {
    static B: OnceLock<Builtin> = OnceLock::new();
    B.get_or_init(|| serde_json::from_str(BUILTIN).expect("defaults/prices.json 格式不对"))
}

/// 存在 `app_settings` 的那份：站主改的（同名盖掉内置的，不同名是加的）、刷新来的、汇率。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    pub fx_rate: Option<f64>,
    pub edits: Vec<PriceRow>,
    pub online: Vec<PriceRow>,
    pub refreshed_at: Option<i64>,
}

/// 现在生效的价格表。
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub fx_rate: f64,
    pub rows: Vec<PriceRow>,
}

fn fx_ok(v: f64) -> bool {
    v.is_finite() && v > 0.1 && v < 100.0
}

impl Book {
    pub fn from_saved(saved: &Saved) -> Self {
        let mut rows: Vec<PriceRow> = builtin().rows.clone();
        for layer in [&saved.online, &saved.edits] {
            for r in layer {
                match rows.iter_mut().find(|x| x.model == r.model) {
                    Some(x) => *x = r.clone(),
                    None => rows.push(r.clone()),
                }
            }
        }
        Self { fx_rate: saved.fx_rate.filter(|v| fx_ok(*v)).unwrap_or(DEFAULT_FX), rows }
    }

    /// 按模型名找：原样、去掉 `[1m]`、`@default`、供应商前缀（`meta/`）、强度后缀（`-medium`）、日期后缀。
    pub fn find(&self, name: &str) -> Option<&PriceRow> {
        candidates(name).into_iter().find_map(|c| self.rows.iter().find(|r| r.model.eq_ignore_ascii_case(&c)))
    }

    /// 这一份用量花了多少美元。`tiers`：按单次请求的提示大小换档（Peck Loop 是；CLI 报的回合合计不是）。`at`：什么时候用的（高峰价）。
    pub fn usd(&self, row: &PriceRow, usage: &TokenUsage, tiers: bool, at: i64) -> f64 {
        let prompt = usage.prompt_total();
        let tier = if tiers { row.tiers.iter().filter(|t| prompt > t.above_tokens).max_by_key(|t| t.above_tokens) } else { None };
        let (input, output, cache_read, cache_write) = match tier {
            Some(t) => (t.input, t.output, t.cache_read, t.cache_write),
            None => (row.input, row.output, row.cache_read, row.cache_write),
        };
        let n = |v: Option<u64>| v.unwrap_or(0) as f64;
        let native = (n(usage.input) * input
            + n(usage.output) * output
            + n(usage.cache_read) * cache_read.unwrap_or(input)
            + n(usage.cache_write) * cache_write.unwrap_or(input))
            / 1_000_000.0;
        let native = match row.peak.filter(|m| m.is_finite() && *m > 0.0) {
            Some(m) if is_peak(at) => native * m,
            _ => native,
        };
        match row.currency {
            Currency::Usd => native,
            Currency::Cny => native / self.fx_rate,
        }
    }

    /// 查得到价格就算（美元）；用量里一个数都没有、查不到：`None`。
    pub fn usd_for(&self, name: &str, usage: &TokenUsage, tiers: bool, at: i64) -> Option<f64> {
        if usage.input.is_none() && usage.output.is_none() {
            return None;
        }
        self.find(name).map(|row| self.usd(row, usage, tiers, at))
    }
}

fn candidates(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().to_string();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    let raw = name.trim();
    push(raw);
    let base = raw.split('[').next().unwrap_or(raw);
    let base = base.split('@').next().unwrap_or(base);
    let base = base.rsplit('/').next().unwrap_or(base);
    push(base);
    let mut cur = base.to_string();
    for _ in 0..2 {
        let lower = cur.to_ascii_lowercase();
        let effort = ["-minimal", "-low", "-medium", "-high", "-xhigh", "-max"].iter().find(|s| lower.ends_with(*s));
        let dated = lower.rsplit_once('-').filter(|(_, d)| d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()));
        if let Some(s) = effort {
            cur.truncate(cur.len() - s.len());
        } else if let Some((head, _)) = dated {
            cur.truncate(head.len());
        } else {
            break;
        }
        push(&cur);
    }
    out
}

fn book_cell() -> &'static RwLock<Book> {
    static BOOK: OnceLock<RwLock<Book>> = OnceLock::new();
    BOOK.get_or_init(|| RwLock::new(Book::from_saved(&Saved::default())))
}

/// 现在生效的价格表（打开 App 时读一次，设置里改了、刷新了就换）。
pub fn book() -> Book {
    book_cell().read().map(|b| b.clone()).unwrap_or_else(|_| Book::from_saved(&Saved::default()))
}

/// Peck Loop 每次请求用：查得到就按价格表（带分档），查不到 `None`（再退回目录里的价格）。
pub fn usd_for(name: &str, usage: &TokenUsage) -> Option<f64> {
    book_cell().read().ok()?.usd_for(name, usage, true, crate::store::now_ms())
}

/// 模型列表里给界面看的价格（原币种）。
pub fn row_for(name: &str) -> Option<PriceRow> {
    book_cell().read().ok()?.find(name).cloned()
}

fn install(saved: &Saved) {
    if let Ok(mut b) = book_cell().write() {
        *b = Book::from_saved(saved);
    }
}

pub fn load_saved(store: &Store) -> Saved {
    store
        .setting(SETTING_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 打开 App 时：读存着的改动，装上。
pub fn load(store: &Store) {
    install(&load_saved(store));
}

fn save(store: &Store, saved: &Saved) -> Result<(), String> {
    store
        .set_setting(SETTING_KEY, &serde_json::to_string(saved).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    install(saved);
    Ok(())
}

// ───────────────────────── 给设置页 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// 内置的。
    Builtin,
    /// 刷新来的（和内置的不一样）。
    Online,
    /// 站主改过的内置那一行。
    Edited,
    /// 站主加的。
    Custom,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceView {
    #[serde(flatten)]
    pub row: PriceRow,
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceBookView {
    pub fx_rate: f64,
    pub default_fx: f64,
    pub vendors: Vec<Vendor>,
    pub rows: Vec<PriceView>,
    /// 内置价格查的日期。
    pub builtin_date: String,
    pub refreshed_at: Option<i64>,
}

pub fn view(saved: &Saved) -> PriceBookView {
    let book = Book::from_saved(saved);
    let in_builtin = |m: &str| builtin().rows.iter().any(|r| r.model == m);
    let rows = book
        .rows
        .into_iter()
        .map(|row| {
            let origin = if saved.edits.iter().any(|r| r.model == row.model) {
                if in_builtin(&row.model) { Origin::Edited } else { Origin::Custom }
            } else if saved.online.iter().any(|r| r.model == row.model) {
                Origin::Online
            } else {
                Origin::Builtin
            };
            PriceView { row, origin }
        })
        .collect();
    PriceBookView {
        fx_rate: book.fx_rate,
        default_fx: DEFAULT_FX,
        vendors: builtin().vendors.clone(),
        rows,
        builtin_date: builtin().date.clone(),
        refreshed_at: saved.refreshed_at,
    }
}

fn valid(row: &PriceRow) -> Result<PriceRow, String> {
    let model = row.model.trim().to_string();
    if model.is_empty() {
        return Err("模型名不能空着。".into());
    }
    let ok = |v: f64| v.is_finite() && v >= 0.0 && v < 10_000.0;
    let opt_ok = |v: Option<f64>| v.is_none_or(ok);
    if !ok(row.input) || !ok(row.output) || !opt_ok(row.cache_read) || !opt_ok(row.cache_write) {
        return Err("价格要是 0 到 10000 之间的数。".into());
    }
    if row.peak.is_some_and(|m| !(m.is_finite() && m > 0.0 && m <= 10.0)) {
        return Err("高峰倍数要在 0 到 10 之间。".into());
    }
    let label = if row.label.trim().is_empty() { model.clone() } else { row.label.trim().to_string() };
    let vendor = if row.vendor.trim().is_empty() { "other".into() } else { row.vendor.trim().to_string() };
    Ok(PriceRow { model, label, vendor, ..row.clone() })
}

/// 改一行（同名的内置行改成这样；没有的就是加一行）。
pub fn set_row(saved: &mut Saved, row: &PriceRow) -> Result<(), String> {
    let row = valid(row)?;
    saved.edits.retain(|r| r.model != row.model);
    // 和内置的一模一样：不算改过。
    if !builtin().rows.iter().any(|r| r == &row) {
        saved.edits.push(row);
    }
    Ok(())
}

/// 退回：改过的回到内置（或刷新来的）；加的删掉。
pub fn reset_row(saved: &mut Saved, model: &str) {
    saved.edits.retain(|r| r.model != model);
}

pub fn set_fx(saved: &mut Saved, fx: f64) -> Result<(), String> {
    if !fx_ok(fx) {
        return Err("汇率要在 0.1 到 100 之间。".into());
    }
    saved.fx_rate = Some(fx);
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    /// 价格变了的。
    pub changed: Vec<String>,
    pub same: usize,
    /// models.dev 上没找到的。
    pub missing: Vec<String>,
    /// 人民币的（不刷新）。
    pub skipped_cny: usize,
}

/// models.dev 的一个模型 → 价格（美元）。
fn row_from_models_dev(base: &PriceRow, cost: &Value) -> Option<PriceRow> {
    let input = cost["input"].as_f64()?;
    let output = cost["output"].as_f64()?;
    let tiers = cost["tiers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["tier"]["type"].as_str() == Some("context"))
        .filter_map(|t| {
            Some(PriceTier {
                above_tokens: t["tier"]["size"].as_u64()?,
                input: t["input"].as_f64()?,
                output: t["output"].as_f64()?,
                cache_read: t["cache_read"].as_f64(),
                cache_write: t["cache_write"].as_f64(),
            })
        })
        .collect();
    Some(PriceRow {
        input,
        output,
        cache_read: cost["cache_read"].as_f64(),
        cache_write: cost["cache_write"].as_f64(),
        tiers,
        ..base.clone()
    })
}

/// 刷新：拿 models.dev 的整份数据，更新美元价（内置的和站主加的；站主改过的那一行不动，改过的就是站主的意思）。
pub fn apply_refresh(saved: &mut Saved, data: &Value, now: i64) -> RefreshReport {
    let mut report = RefreshReport::default();
    let vendors = &builtin().vendors;
    let mut online: Vec<PriceRow> = Vec::new();
    let base_rows: Vec<PriceRow> = builtin().rows.iter().cloned().chain(saved.edits.iter().filter(|e| !builtin().rows.iter().any(|b| b.model == e.model)).cloned()).collect();
    for base in base_rows {
        if base.currency == Currency::Cny {
            report.skipped_cny += 1;
            continue;
        }
        let edited = saved.edits.iter().any(|e| e.model == base.model);
        let Some(provider) = vendors.iter().find(|v| v.id == base.vendor).and_then(|v| v.models_dev.clone()) else {
            report.missing.push(base.label.clone());
            continue;
        };
        let cost = &data[&provider]["models"][&base.model]["cost"];
        let Some(fresh) = row_from_models_dev(&base, cost) else {
            report.missing.push(base.label.clone());
            continue;
        };
        let builtin_row = builtin().rows.iter().find(|r| r.model == base.model);
        let was = saved.online.iter().find(|r| r.model == base.model).or(builtin_row).unwrap_or(&base);
        if prices_equal(was, &fresh) {
            report.same += 1;
        } else {
            report.changed.push(base.label.clone());
        }
        // 和内置的一样就不用另存。站主改过的那一行：照样记下最新的，但生效的还是站主的。
        if builtin_row.is_none_or(|b| !prices_equal(b, &fresh)) && !(edited && builtin_row.is_none()) {
            online.push(fresh);
        }
    }
    saved.online = online;
    saved.refreshed_at = Some(now);
    report
}

fn prices_equal(a: &PriceRow, b: &PriceRow) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 1e-9;
    let close_o = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) => close(x, y),
        (None, None) => true,
        _ => false,
    };
    close(a.input, b.input)
        && close(a.output, b.output)
        && close_o(a.cache_read, b.cache_read)
        && close_o(a.cache_write, b.cache_write)
        && a.tiers.len() == b.tiers.len()
        && a.tiers.iter().zip(&b.tiers).all(|(x, y)| x.above_tokens == y.above_tokens && close(x.input, y.input) && close(x.output, y.output) && close_o(x.cache_read, y.cache_read))
}

// ───────────────────────── IPC ─────────────────────────

#[tauri::command]
pub fn prices_get(state: tauri::State<'_, crate::state::AppState>) -> Result<PriceBookView, String> {
    let store = state.store.lock().map_err(|_| "读不到价格表。")?;
    Ok(view(&load_saved(&store)))
}

/// 改一行、退回一行、改汇率（一次一样）。
#[tauri::command]
pub fn prices_set(
    row: Option<PriceRow>,
    reset: Option<String>,
    fx_rate: Option<f64>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<PriceBookView, String> {
    let store = state.store.lock().map_err(|_| "存不了价格表。")?;
    let mut saved = load_saved(&store);
    if let Some(row) = row {
        set_row(&mut saved, &row)?;
    }
    if let Some(model) = reset {
        reset_row(&mut saved, &model);
    }
    if let Some(fx) = fx_rate {
        set_fx(&mut saved, fx)?;
    }
    save(&store, &saved)?;
    Ok(view(&saved))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    pub book: PriceBookView,
    pub report: RefreshReport,
}

/// REFRESH：从 models.dev 取一次（只读公开的价格数据，不带任何账号信息）。
#[tauri::command]
pub async fn prices_refresh(state: tauri::State<'_, crate::state::AppState>) -> Result<RefreshResult, String> {
    let client = crate::api::http_client()?;
    let resp = client.get(MODELS_DEV).send().await.map_err(|_| "连不上 models.dev，过会儿再试。".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("models.dev 回了 {}，过会儿再试。", resp.status().as_u16()));
    }
    let data: Value = resp.json().await.map_err(|_| "models.dev 给的数据读不懂。".to_string())?;
    let store = state.store.lock().map_err(|_| "存不了价格表。")?;
    let mut saved = load_saved(&store);
    let report = apply_refresh(&mut saved, &data, crate::store::now_ms());
    save(&store, &saved)?;
    Ok(RefreshResult { book: view(&saved), report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn usage(input: u64, output: u64, cache_read: u64) -> TokenUsage {
        TokenUsage { input: Some(input), output: Some(output), cache_read: Some(cache_read), ..Default::default() }
    }

    #[test]
    fn builtin_parses_and_covers_the_catalog_and_cli_models() {
        let book = Book::from_saved(&Saved::default());
        assert_eq!(book.fx_rate, DEFAULT_FX);
        for m in &super::super::catalog::catalog().models {
            assert!(book.find(&m.api_model).is_some(), "价格表里没有 {}", m.id);
        }
        // CLI 报的模型名（2026-11 站主正式数据里见过的）。
        for name in ["claude-haiku-5-5", "gpt-6-luna", "gpt-6.1-sol", "grok-4.6", "grok-4.7", "muse-wonder-1.0", "gemini-4-argon-medium", "claude-opus-5-5[1m]", "meta/muse-spark-1.3"] {
            assert!(book.find(name).is_some(), "价格表里找不到 {name}");
        }
        for r in &book.rows {
            assert!(builtin().vendors.iter().any(|v| v.id == r.vendor), "{} 的 vendor 不在列表里", r.model);
            let cny = matches!(r.vendor.as_str(), "deepseek" | "aliyun");
            assert_eq!(r.currency == Currency::Cny, cny, "{} 的币种", r.model);
        }
    }

    #[test]
    fn names_are_matched_loosely() {
        assert_eq!(candidates("gemini-4-argon-medium"), ["gemini-4-argon-medium", "gemini-4-argon"]);
        assert_eq!(candidates("vertex/claude-opus-5-5@default"), ["vertex/claude-opus-5-5@default", "claude-opus-5-5"]);
        assert_eq!(candidates("claude-sonnet-5-5-20260928"), ["claude-sonnet-5-5-20260928", "claude-sonnet-5-5"]);
    }

    #[test]
    fn yuan_prices_are_converted_with_the_rate() {
        let row = PriceRow { model: "x".into(), label: "X".into(), vendor: "deepseek".into(), currency: Currency::Cny, input: 2.0, output: 8.0, cache_read: Some(0.2), cache_write: None, tiers: vec![], peak: Some(2.0) };
        let book = Book { fx_rate: 8.0, rows: vec![row.clone()] };
        // 2026-11-15 是周日（北京时间 10 点）：不是高峰。(100 万 × 2 + 50 万 × 8 + 100 万 × 0.2) / 百万 = ¥6.2 → $0.775
        let sunday_10 = 1_794_708_000_000;
        assert!(!is_peak(sunday_10));
        assert!((book.usd(&row, &usage(1_000_000, 500_000, 1_000_000), true, sunday_10) - 0.775).abs() < 1e-9);
        // 周一北京时间 10 点：高峰，2 倍。
        let monday_10 = sunday_10 + 86_400_000;
        assert!(is_peak(monday_10) && !is_peak(monday_10 + 3 * 3_600_000) && is_peak(monday_10 + 5 * 3_600_000));
        assert!((book.usd(&row, &usage(1_000_000, 500_000, 1_000_000), true, monday_10) - 1.55).abs() < 1e-9);
    }

    #[test]
    fn edits_win_and_reset_goes_back() {
        let mut saved = Saved::default();
        let mut row = Book::from_saved(&saved).find("gpt-6-luna").unwrap().clone();
        row.input = 9.0;
        set_row(&mut saved, &row).unwrap();
        assert_eq!(Book::from_saved(&saved).find("gpt-6-luna").unwrap().input, 9.0);
        let v = view(&saved);
        assert_eq!(v.rows.iter().find(|r| r.row.model == "gpt-6-luna").unwrap().origin, Origin::Edited);
        reset_row(&mut saved, "gpt-6-luna");
        assert!(saved.edits.is_empty());
        // 加一行自己的。
        set_row(&mut saved, &PriceRow { model: "my-model".into(), label: "".into(), vendor: "".into(), currency: Currency::Usd, input: 1.0, output: 2.0, cache_read: None, cache_write: None, tiers: vec![], peak: None }).unwrap();
        assert_eq!(view(&saved).rows.iter().find(|r| r.row.model == "my-model").unwrap().origin, Origin::Custom);
        assert!(set_row(&mut saved, &PriceRow { input: -1.0, ..row }).is_err());
        assert!(set_fx(&mut saved, 0.0).is_err());
        set_fx(&mut saved, 7.1).unwrap();
        assert_eq!(Book::from_saved(&saved).fx_rate, 7.1);
    }

    #[test]
    fn refresh_updates_dollar_rows_and_leaves_yuan_and_edits() {
        let mut saved = Saved::default();
        let luna = Book::from_saved(&saved).find("gpt-6-luna").unwrap().clone();
        let mut edited = Book::from_saved(&saved).find("claude-opus-5-5").unwrap().clone();
        edited.output = 99.0;
        set_row(&mut saved, &edited).unwrap();
        let data = json!({
            "openai": { "models": { "gpt-6-luna": { "cost": { "input": 0.05, "output": 0.5, "cache_read": 0.005, "tiers": [{ "tier": { "type": "context", "size": 272000 }, "input": 0.1, "output": 1.0 }] } } } },
            "anthropic": { "models": { "claude-opus-5-5": { "cost": { "input": 3.0, "output": 15.0 } } } },
        });
        let report = apply_refresh(&mut saved, &data, 42);
        assert!(report.changed.contains(&luna.label));
        assert!(report.skipped_cny > 0);
        assert_eq!(saved.refreshed_at, Some(42));
        let book = Book::from_saved(&saved);
        assert_eq!(book.find("gpt-6-luna").unwrap().input, 0.05);
        assert_eq!(book.find("gpt-6-luna").unwrap().tiers[0].above_tokens, 272_000);
        // 改过的还是站主的。
        assert_eq!(book.find("claude-opus-5-5").unwrap().output, 99.0);
        assert_eq!(view(&saved).rows.iter().find(|r| r.row.model == "gpt-6-luna").unwrap().origin, Origin::Online);
    }
}
