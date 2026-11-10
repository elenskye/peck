use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Anthropic,
    ChatCompletions,
    Responses,
    Gemini,
    Qwen,
}

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
    pub base_url: String,
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    Unknown,
    Never,
    Switch,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub api_model: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub thinking: Thinking,
    pub efforts: Vec<String>,
    pub tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub role: String,
    pub model_id: String,
    pub thinking: bool,
    pub effort: Option<String>,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub models: Vec<ModelProfile>,
    pub agents: Vec<AgentProfile>,
    #[serde(default)]
    pub aliyun_region: AliyunRegion,
    #[serde(default)]
    pub aliyun_workspace_id: String,
}

pub fn providers() -> Vec<Provider> {
    vec![
        Provider {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            protocol: Protocol::Anthropic,
        },
        Provider {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            protocol: Protocol::ChatCompletions,
        },
        Provider {
            id: "openai".into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            protocol: Protocol::Responses,
        },
        Provider {
            id: "google".into(),
            name: "Google Gemini".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            protocol: Protocol::Gemini,
        },
        Provider {
            id: "aliyun".into(),
            name: "阿里云百炼".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            protocol: Protocol::Qwen,
        },
    ]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            aliyun_region: AliyunRegion::default(),
            aliyun_workspace_id: String::new(),
            models: vec![
                ModelProfile {
                    id: "opus".into(),
                    name: "Claude Opus 5.5".into(),
                    provider_id: "anthropic".into(),
                    api_model: "claude-opus-5-5".into(),
                    context_window: 1_000_000,
                    max_output_tokens: 128000,
                    thinking: Thinking::Always,
                    efforts: ["low", "medium", "high", "xhigh", "max"]
                        .map(String::from)
                        .to_vec(),
                    tools: true,
                },
                ModelProfile {
                    id: "flash".into(),
                    name: "DeepSeek Flash".into(),
                    provider_id: "deepseek".into(),
                    api_model: "deepseek-flash".into(),
                    context_window: 1_000_000,
                    max_output_tokens: 393216,
                    thinking: Thinking::Switch,
                    efforts: ["low", "high", "max"].map(String::from).to_vec(),
                    tools: true,
                },
            ],
            agents: vec![
                AgentProfile {
                    role: "supervisor".into(),
                    model_id: "opus".into(),
                    thinking: true,
                    effort: Some("medium".into()),
                    max_output_tokens: 4096,
                },
                AgentProfile {
                    role: "worker".into(),
                    model_id: "flash".into(),
                    thinking: true,
                    effort: Some("high".into()),
                    max_output_tokens: 4096,
                },
            ],
        }
    }
}

impl Config {
    pub fn providers(&self) -> Vec<Provider> {
        let mut result = providers();
        let aliyun = result.iter_mut().find(|p| p.id == "aliyun").unwrap();
        aliyun.base_url = if self.aliyun_workspace_id.is_empty() {
            match self.aliyun_region {
                AliyunRegion::Beijing => "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                AliyunRegion::Singapore => {
                    "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into()
                }
            }
        } else {
            let region = match self.aliyun_region {
                AliyunRegion::Beijing => "cn-beijing",
                AliyunRegion::Singapore => "ap-southeast-1",
            };
            format!(
                "https://{}.{region}.maas.aliyuncs.com/compatible-mode/v1",
                self.aliyun_workspace_id
            )
        };
        result
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.aliyun_workspace_id.len() > 100
            || self
                .aliyun_workspace_id
                .bytes()
                .any(|b| !b.is_ascii_alphanumeric() && b != b'-')
        {
            return Err("阿里云业务空间 ID 只能包含英文字母、数字和连字符。".into());
        }
        if self.models.is_empty() || self.models.len() > 30 {
            return Err("请保留 1～30 个模型配置。".into());
        }
        let defaults = Config::default();
        let mut ids = std::collections::HashSet::new();
        for m in &self.models {
            if m.id.is_empty()
                || !ids.insert(&m.id)
                || m.name.trim().is_empty()
                || m.api_model.is_empty()
                || m.api_model.len() > 160
                || m.api_model.bytes().any(|b| b <= 32 || b >= 127)
                || !providers().iter().any(|p| p.id == m.provider_id)
                || m.context_window < 1024
                || m.max_output_tokens < 64
                || m.max_output_tokens > m.context_window
            {
                return Err("模型名称、ID、供应商或 token 上限无效。".into());
            }
            if m.efforts.iter().any(|e| {
                !["minimal", "low", "medium", "high", "xhigh", "max"].contains(&e.as_str())
            }) {
                return Err("思考档位必须使用供应商支持的原生值。".into());
            }
            if m.provider_id == "aliyun" && !m.efforts.is_empty() {
                return Err("阿里云百炼使用思考开关，强度档位请留空。".into());
            }
            if m.provider_id == "google"
                && m.efforts
                    .iter()
                    .any(|e| !["minimal", "low", "medium", "high"].contains(&e.as_str()))
            {
                return Err("Google 兼容接口的原生强度仅支持 minimal、low、medium、high。".into());
            }
            if let Some(known) = defaults
                .models
                .iter()
                .find(|d| d.provider_id == m.provider_id && d.api_model == m.api_model)
            {
                if known.thinking != m.thinking
                    || known.efforts != m.efforts
                    || !m.tools
                    || m.max_output_tokens > known.max_output_tokens
                    || m.context_window > known.context_window
                {
                    return Err("已知模型的能力与预设不符；请恢复该模型的能力设置。".into());
                }
            }
        }
        if self.agents.len() != 2 {
            return Err("请配置 Supervisor 和 Worker 两个角色。".into());
        }
        for role in ["supervisor", "worker"] {
            let matches: Vec<_> = self.agents.iter().filter(|a| a.role == role).collect();
            if matches.len() != 1 {
                return Err("角色配置重复或缺失。".into());
            }
            let a = matches[0];
            let m = self
                .models
                .iter()
                .find(|m| m.id == a.model_id)
                .ok_or("请选择角色使用的模型。")?;
            if a.max_output_tokens < 64 || a.max_output_tokens > m.max_output_tokens {
                return Err("角色输出上限超出模型允许范围。".into());
            }
            if (m.thinking == Thinking::Always && !a.thinking)
                || (m.thinking == Thinking::Never && a.thinking)
            {
                return Err("思考开关与模型能力不符。".into());
            }
            if let Some(e) = &a.effort {
                if !a.thinking || !m.efforts.contains(e) {
                    return Err("所选模型不支持该思考档位。".into());
                }
            }
        }
        Ok(())
    }

    pub fn selection(&self, role: &str) -> Result<(AgentProfile, ModelProfile, Provider), String> {
        self.validate()?;
        let a = self
            .agents
            .iter()
            .find(|a| a.role == role)
            .ok_or("未知角色。")?
            .clone();
        let m = self
            .models
            .iter()
            .find(|m| m.id == a.model_id)
            .ok_or("模型不存在。")?
            .clone();
        if m.thinking == Thinking::Unknown {
            return Err("此模型能力尚未确认，请先设置思考能力。".into());
        }
        let p = self
            .providers()
            .into_iter()
            .find(|p| p.id == m.provider_id)
            .ok_or("供应商不存在。")?;
        Ok((a, m, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_and_aliyun_region_keep_official_destinations() {
        let mut legacy = serde_json::to_value(Config::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("aliyunRegion");
        legacy.as_object_mut().unwrap().remove("aliyunWorkspaceId");
        let mut config: Config = serde_json::from_value(legacy).unwrap();
        config.validate().unwrap();
        assert_eq!(config.providers().len(), 5);
        config.aliyun_region = AliyunRegion::Singapore;
        config.aliyun_workspace_id = "workspace-fixture".into();
        config.models[1].provider_id = "aliyun".into();
        config.models[1].api_model = "fixture-model".into();
        config.models[1].efforts.clear();
        config.agents[1].effort = None;
        let (_, _, provider) = config.selection("worker").unwrap();
        assert_eq!(
            provider.base_url,
            "https://workspace-fixture.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1"
        );
        config.aliyun_workspace_id = "bad/path@elsewhere".into();
        assert!(config.validate().is_err());
    }
}
