//! 模型目录和价格：`defaults/models.json`（`scripts/models-snapshot.mjs` 从 models.dev 取的快照），
//! 和给界面的模型列表（`gateway_models`）。
//!
//! 花费：API 按实报用量 × 价格表算（`priced`）；没价格的标 `unknown`，不填 0。

use super::caps::AgentCaps;
use super::event::{Cost, CostSource, TokenUsage, Wire};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const SNAPSHOT: &str = include_str!("../../defaults/models.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub snapshot: SnapshotInfo,
    pub providers: Vec<ProviderSpec>,
    pub models: Vec<ModelSpec>,
    pub engines: Vec<EngineSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub source: String,
    /// 价格的日期。
    pub date: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    /// 钥匙串里的 Key。
    Key,
    /// gcloud 的 ADC（Vertex）。
    Adc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSpec {
    pub id: String,
    pub label: String,
    pub wire: Wire,
    /// Chat Completions 的方言：`deepseek`、`qwen`。
    #[serde(default)]
    pub dialect: Option<String>,
    pub auth: Auth,
    /// Vertex 的地址按项目和区域拼（`config.rs`），这里为空。
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub region_base_urls: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Never,
    /// 能开能关。
    Switch,
    /// 一直开着。
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    /// `供应商/模型`，全目录唯一。
    pub id: String,
    pub provider: String,
    /// 发给供应商的模型名。
    pub api_model: String,
    pub label: String,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output: Option<u64>,
    pub thinking: Thinking,
    #[serde(default)]
    pub efforts: Vec<String>,
    pub tools: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub price: Option<Price>,
    /// 百炼这类按地域不同价。
    #[serde(default)]
    pub region_prices: HashMap<String, Price>,
}

/// 每百万 tokens 的美元价格。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    /// 提示超过 `above_tokens` 以后整条请求按这一档算。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<PriceTier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceTier {
    pub above_tokens: u64,
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSpec {
    pub id: String,
    pub label: String,
    /// 可执行文件名。
    pub command: String,
    /// `app-server`、`stream-json`、`exec-json`、`acp`。
    pub protocol: String,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(SNAPSHOT).expect("defaults/models.json 格式不对"))
}

impl Catalog {
    pub fn model(&self, id: &str) -> Option<&ModelSpec> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderSpec> {
        self.providers.iter().find(|p| p.id == id)
    }
}

impl Price {
    /// 按用量算一次请求的花费。缓存读写没单价的按输入价算。
    pub fn cost(&self, usage: &TokenUsage) -> f64 {
        let prompt = usage.prompt_total();
        let (input, output, cache_read, cache_write) = match self
            .tiers
            .iter()
            .filter(|t| prompt > t.above_tokens)
            .max_by_key(|t| t.above_tokens)
        {
            Some(t) => (t.input, t.output, t.cache_read, t.cache_write),
            None => (self.input, self.output, self.cache_read, self.cache_write),
        };
        let tokens = |n: Option<u64>| n.unwrap_or(0) as f64;
        (tokens(usage.input) * input
            + tokens(usage.output) * output
            + tokens(usage.cache_read) * cache_read.unwrap_or(input)
            + tokens(usage.cache_write) * cache_write.unwrap_or(input))
            / 1_000_000.0
    }
}

impl ModelSpec {
    pub fn price_for(&self, region: Option<&str>) -> Option<&Price> {
        region
            .and_then(|r| self.region_prices.get(r))
            .or(self.price.as_ref())
    }

    /// 用量 × 价格表。没价格或用量里一个数都没有：`unknown`。
    pub fn cost(&self, usage: &TokenUsage, region: Option<&str>) -> Cost {
        let has_numbers = usage.input.is_some() || usage.output.is_some();
        match self.price_for(region) {
            Some(price) if has_numbers => Cost {
                usd: Some(price.cost(usage)),
                source: CostSource::Priced,
            },
            _ => Cost::unknown(),
        }
    }
}

// ───────────────────────── 模型列表 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryGroup {
    /// Peck 自己的循环 + API 模型。
    Peck,
    /// CLI 引擎。
    Cli,
    /// 现有的接力和规划 + 实施（v0.8 并进多引擎合作）。
    Flow,
}

/// 模型列表里的一项。`id` 就是发任务时的「谁来干」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    /// `api:anthropic/claude-opus-5-5`、`cli:codex`、`flow:muse>grok`。
    pub id: String,
    pub group: EntryGroup,
    pub label: String,
    /// 供应商或引擎的 id，和它的显示名（列表按它分组）。
    pub provider: String,
    pub provider_label: String,
    pub available: bool,
    /// 不能用时写原因：「没配 Key」「没检测到」「先跑 gcloud auth application-default login」。
    #[serde(default)]
    pub unavailable_reason: Option<String>,
    /// 只做直接调用（Antigravity）：列表里标出来，不进 ONE SHOT 和自动化。
    pub direct_only: bool,
    #[serde(default)]
    pub thinking: Option<Thinking>,
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub price: Option<Price>,
    pub caps: AgentCaps,
    /// CLI 的版本。
    #[serde(default)]
    pub version: Option<String>,
    /// CLI 引擎当前用的账号（设置里起的名字，比如「工作」）。API 和没配账号的为空。
    #[serde(default)]
    pub account: Option<String>,
    /// 转换器按哪个版本的 CLI 写的（录夹具时的版本）。和 `version` 里的版本号不一样就提示「格式可能变了」。
    #[serde(default)]
    pub tested_version: Option<String>,
}

/// 录夹具时各家 CLI 的版本（2026-11-12）。升级 CLI 后重录夹具、确认转换器没问题再改这里。
pub const TESTED_VERSIONS: [(&str, &str); 5] = [
    ("claude-code", "2.1.322"),
    ("codex", "0.167.2"),
    ("muse", "1.4.15"),
    ("grok", "1.0.77"),
    ("antigravity", "1.5.6"),
];

pub fn tested_version(engine: &str) -> Option<String> {
    TESTED_VERSIONS.iter().find(|(e, _)| *e == engine).map(|(_, v)| v.to_string())
}

/// `codex-cli 0.167.2`、`Muse Code 1.4.15 (…)` 这类 → 第一个 `x.y.z`。
pub fn version_number(raw: &str) -> Option<String> {
    raw.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|w| w.split('.').filter(|p| !p.is_empty()).count() >= 2 && w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|w| w.trim_end_matches('.').to_string())
}

/// 哪些东西能用。
#[derive(Debug, Clone, Default)]
pub struct Availability {
    /// `?mock` 和测试：全部当能用。
    pub all: bool,
    /// (供应商 id, 钥匙串里有没有 Key)
    pub keys: Vec<(String, bool)>,
    /// ADC 在、Project ID 填了。
    pub vertex_ready: bool,
    pub vertex_adc: bool,
    pub engines: Vec<EngineProbe>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineProbe {
    pub id: String,
    pub found: bool,
    pub version: Option<String>,
}

impl EngineProbe {
    pub fn all_found() -> Vec<Self> {
        catalog()
            .engines
            .iter()
            .map(|e| Self {
                id: e.id.clone(),
                found: true,
                version: None,
            })
            .collect()
    }

    /// 在设置里的「CLI 目录」找，没设就照 PATH 找；找到了顺手取版本。
    pub async fn detect(config: &super::config::GatewayConfig) -> Vec<Self> {
        let extra: Vec<String> = config
            .cli_dir_path()
            .map(|p| vec![p.to_string_lossy().into_owned()])
            .unwrap_or_default();
        let path = crate::agent::build_augmented_path(&extra);
        let mut out = Vec::new();
        for engine in &catalog().engines {
            let resolved = crate::agent::resolve_command_path(&engine.command, &path).await;
            let version = match &resolved {
                Some(bin) => crate::agent::probe_command_version(bin, &path, &HashMap::new())
                    .await
                    .ok(),
                None => None,
            };
            out.push(Self {
                id: engine.id.clone(),
                found: resolved.is_some(),
                version,
            });
        }
        out
    }
}

pub fn model_entries(catalog: &Catalog, available: &Availability) -> Vec<ModelEntry> {
    let mut out = Vec::new();
    for model in &catalog.models {
        let Some(provider) = catalog.provider(&model.provider) else {
            continue;
        };
        let reason = if available.all {
            None
        } else {
            match provider.auth {
                Auth::Key => {
                    let saved = available
                        .keys
                        .iter()
                        .any(|(id, saved)| id == &provider.id && *saved);
                    (!saved).then(|| "没配 Key".to_string())
                }
                Auth::Adc if !available.vertex_adc => {
                    Some("先跑 gcloud auth application-default login".to_string())
                }
                Auth::Adc if !available.vertex_ready => {
                    Some("设置里填 Vertex AI 的 Project ID".to_string())
                }
                Auth::Adc => None,
            }
        };
        out.push(ModelEntry {
            id: format!("api:{}", model.id),
            group: EntryGroup::Peck,
            label: model.label.clone(),
            provider: provider.id.clone(),
            provider_label: provider.label.clone(),
            available: reason.is_none(),
            unavailable_reason: reason,
            direct_only: false,
            thinking: Some(model.thinking),
            efforts: model.efforts.clone(),
            context_window: model.context_window,
            price: model.price.clone(),
            caps: AgentCaps::peck_loop(),
            version: None,
            account: None,
            tested_version: None,
        });
    }
    for engine in &catalog.engines {
        let probe = available.engines.iter().find(|p| p.id == engine.id);
        let found = available.all || probe.is_some_and(|p| p.found);
        let caps = AgentCaps::for_engine(&engine.id);
        out.push(ModelEntry {
            id: format!("cli:{}", engine.id),
            group: EntryGroup::Cli,
            label: engine.label.clone(),
            provider: engine.id.clone(),
            provider_label: engine.label.clone(),
            available: found,
            unavailable_reason: (!found).then(|| "没检测到".to_string()),
            direct_only: caps.direct_only,
            thinking: None,
            efforts: Vec::new(),
            context_window: None,
            price: None,
            caps,
            version: probe.and_then(|p| p.version.clone()),
            account: None,
            tested_version: tested_version(&engine.id),
        });
    }
    // 现有的接力：两家 CLI 都在才能用。规划 + 实施要 API（用现有的「API 设置」里的两个角色）。
    let found = |id: &str| {
        available.all
            || available
                .engines
                .iter()
                .any(|p| p.id == id && p.found)
    };
    for (who, label, a, b) in [
        ("muse>grok", "Muse → Grok", "muse", "grok"),
        ("grok>muse", "Grok → Muse", "grok", "muse"),
    ] {
        let ok = found(a) && found(b);
        out.push(ModelEntry {
            id: format!("flow:{who}"),
            group: EntryGroup::Flow,
            label: label.into(),
            provider: "flow".into(),
            provider_label: "接力".into(),
            available: ok,
            unavailable_reason: (!ok).then(|| "两家 CLI 都要检测到".to_string()),
            direct_only: false,
            thinking: None,
            efforts: Vec::new(),
            context_window: None,
            price: None,
            caps: AgentCaps::for_engine(a),
            version: None,
            account: None,
            tested_version: None,
        });
    }
    out.push(ModelEntry {
        id: "flow:plan-implement".into(),
        group: EntryGroup::Flow,
        label: "规划 + 实施".into(),
        provider: "flow".into(),
        provider_label: "接力".into(),
        available: true,
        unavailable_reason: None,
        direct_only: false,
        thinking: None,
        efforts: Vec::new(),
        context_window: None,
        price: None,
        caps: AgentCaps::peck_loop(),
        version: None,
        account: None,
        tested_version: None,
    });
    out
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn version_numbers_come_out_of_cli_banners() {
        assert_eq!(version_number("codex-cli 0.167.2").as_deref(), Some("0.167.2"));
        assert_eq!(version_number("Muse Code 1.4.15 (1.4.15-R8339.1)").as_deref(), Some("1.4.15"));
        assert_eq!(version_number("grok 1.0.77 (50ca579fa996)").as_deref(), Some("1.0.77"));
        assert_eq!(version_number("2.1.322 (Claude Code)").as_deref(), Some("2.1.322"));
        assert_eq!(version_number("no version"), None);
        assert_eq!(tested_version("antigravity").as_deref(), Some("1.5.6"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_parses_and_every_model_has_a_provider() {
        let c = catalog();
        assert!(!c.models.is_empty());
        for m in &c.models {
            let p = c.provider(&m.provider).unwrap_or_else(|| panic!("{} 没有供应商", m.id));
            assert_eq!(m.id, format!("{}/{}", p.id, m.api_model));
            assert!(m.price.is_some(), "{} 没价格", m.id);
        }
        for id in ["anthropic", "openai", "deepseek", "aliyun", "google", "vertex-anthropic", "vertex-google"] {
            assert!(c.provider(id).is_some(), "少了供应商 {id}");
        }
        let engines: Vec<&str> = c.engines.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(engines, ["claude-code", "codex", "muse", "grok", "antigravity"]);
    }

    #[test]
    fn cost_uses_cache_prices_and_context_tiers() {
        let price = Price {
            input: 1.0,
            output: 5.0,
            cache_read: Some(0.1),
            cache_write: None,
            tiers: vec![PriceTier {
                above_tokens: 272_000,
                input: 2.0,
                output: 7.5,
                cache_read: Some(0.2),
                cache_write: None,
            }],
        };
        // 提示 25.1 万，没过 27.2 万那一档。
        let small = TokenUsage {
            input: Some(100_000),
            output: Some(10_000),
            cache_read: Some(150_000),
            cache_write: Some(1_000),
            ..Default::default()
        };
        // (10万×1 + 1万×5 + 15万×0.1 + 1千×1（缓存写没单价按输入价）) / 百万
        assert!((price.cost(&small) - 0.166).abs() < 1e-9);
        let big = TokenUsage {
            input: Some(300_000),
            output: Some(0),
            ..Default::default()
        };
        assert!((price.cost(&big) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn unknown_cost_is_not_zero() {
        let mut model = catalog().models[0].clone();
        let cost = model.cost(&TokenUsage::default(), None);
        assert_eq!(cost.usd, None);
        assert_eq!(cost.source, CostSource::Unknown);
        model.price = None;
        let usage = TokenUsage {
            input: Some(10),
            output: Some(10),
            ..Default::default()
        };
        assert_eq!(model.cost(&usage, None).source, CostSource::Unknown);
    }

    #[test]
    fn region_price_wins_when_present() {
        let qwen = catalog().model("aliyun/qwen3.8-max").unwrap();
        let usage = TokenUsage {
            input: Some(1_000_000),
            output: Some(0),
            ..Default::default()
        };
        let beijing = qwen.cost(&usage, None).usd.unwrap();
        let singapore = qwen.cost(&usage, Some("singapore")).usd.unwrap();
        assert_ne!(beijing, singapore);
    }

    /// `?mock` 的模型列表就是这份（`src/dev/gatewayMock.ts` 读它），和这里算出来的一样。
    #[test]
    fn mock_model_list_fixture_is_current() {
        let entries = model_entries(catalog(), &Availability { all: true, ..Default::default() });
        let actual = serde_json::to_string_pretty(&entries).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gateway/models-mock.json");
        if std::env::var_os("PECK_REGEN_FIXTURES").is_some() {
            std::fs::write(&path, &actual).unwrap();
            return;
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap_or_default(),
            actual,
            "models-mock.json 过期了：PECK_REGEN_FIXTURES=1 cargo test gateway"
        );
    }

    #[test]
    fn entries_mark_missing_keys_adc_and_direct_only() {
        let entries = model_entries(
            catalog(),
            &Availability {
                all: false,
                keys: vec![("anthropic".into(), true), ("openai".into(), false)],
                vertex_ready: false,
                vertex_adc: true,
                engines: vec![
                    EngineProbe { id: "codex".into(), found: true, version: Some("0.167.2".into()) },
                    EngineProbe { id: "antigravity".into(), found: true, version: None },
                ],
            },
        );
        let get = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
        assert!(get("api:anthropic/claude-opus-5-5").available);
        assert_eq!(
            get("api:openai/gpt-6.1-sol").unavailable_reason.as_deref(),
            Some("没配 Key")
        );
        assert_eq!(
            get("api:vertex-anthropic/claude-opus-5-5@default").unavailable_reason.as_deref(),
            Some("设置里填 Vertex AI 的 Project ID")
        );
        assert!(get("cli:codex").available);
        assert_eq!(get("cli:codex").version.as_deref(), Some("0.167.2"));
        assert_eq!(get("cli:codex").tested_version.as_deref(), Some("0.167.2"));
        assert_eq!(get("api:anthropic/claude-haiku-5-5").tested_version, None);
        assert!(!get("cli:grok").available);
        assert!(get("cli:antigravity").direct_only);
        assert!(!get("flow:muse>grok").available);
        assert!(get("flow:plan-implement").available);
    }
}
