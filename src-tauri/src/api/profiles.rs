//! 设置 → PROVIDERS 那几家（存 Key 用）和 Aliyun 的地域。请求怎么发在 Gateway 的模型目录里（`gateway/catalog.rs`）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AliyunRegion {
    #[default]
    Beijing,
    Singapore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
}

/// 存在 `api_settings` 那一行的设置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub aliyun_region: AliyunRegion,
}

pub fn providers() -> Vec<Provider> {
    [("anthropic", "Anthropic"), ("deepseek", "DeepSeek"), ("openai", "OpenAI"), ("google", "Google Gemini"), ("aliyun", "Aliyun")]
        .into_iter()
        .map(|(id, name)| Provider { id: id.into(), name: name.into() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saved_config_still_reads() {
        let old = serde_json::json!({"models": [], "agents": [], "aliyunRegion": "singapore", "aliyunWorkspaceId": "ws"});
        let config: Config = serde_json::from_value(old).unwrap();
        assert!(matches!(config.aliyun_region, AliyunRegion::Singapore));
        let empty: Config = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(matches!(empty.aliyun_region, AliyunRegion::Beijing));
        assert_eq!(providers().len(), 5);
    }
}
