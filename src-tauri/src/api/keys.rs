use std::collections::HashMap;
use zeroize::Zeroizing;

// Ordinary settings and IPC status never contain credentials.
pub struct Keys {
    service: Option<String>,
    values: Option<HashMap<String, Zeroizing<String>>>,
    failed: bool,
    revision: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub provider_id: String,
    pub state: String,
}

impl Keys {
    pub fn new(service: Option<String>) -> Self {
        let values = if service.is_none() {
            Some(HashMap::new())
        } else {
            None
        };
        Self {
            service,
            values,
            failed: false,
            revision: 0,
        }
    }

    pub fn status(&self) -> Vec<KeyStatus> {
        super::profiles::providers()
            .iter()
            .map(|p| KeyStatus {
                provider_id: p.id.clone(),
                state: if self.failed {
                    "unavailable"
                } else if let Some(v) = &self.values {
                    if v.contains_key(&p.id) {
                        "saved"
                    } else {
                        "missing"
                    }
                } else {
                    "unloaded"
                }
                .into(),
            })
            .collect()
    }

    pub fn load(&mut self) -> Result<(), String> {
        if self.values.is_some() {
            return Ok(());
        }
        let result = match &self.service {
            None => Ok(HashMap::new()),
            Some(service) => read(service),
        };
        match result {
            Ok(values) => {
                self.values = Some(values);
                self.failed = false;
                self.revision += 1;
                Ok(())
            }
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }

    pub fn set(&mut self, provider: &str, key: Option<String>) -> Result<(), String> {
        if !super::profiles::providers()
            .iter()
            .any(|p| p.id == provider)
        {
            return Err("未知供应商。".into());
        }
        let key = key.map(|s| {
            let s = Zeroizing::new(s);
            Zeroizing::new(s.trim().to_owned())
        });
        if key.as_ref().is_some_and(|s| {
            s.is_empty() || s.len() > 4096 || s.bytes().any(|b| !(33..=126).contains(&b))
        }) {
            return Err("Key 不能为空或包含空格、换行。".into());
        }
        self.load()?;
        let mut values = self.values.as_ref().unwrap().clone();
        if let Some(key) = key {
            values.insert(provider.into(), key);
        } else {
            values.remove(provider);
        }
        if let Some(service) = &self.service {
            write(service, &values)?;
        }
        self.values = Some(values);
        self.revision += 1;
        Ok(())
    }

    pub fn cached(&self, provider: &str) -> Option<Zeroizing<String>> {
        self.values.as_ref()?.get(provider).cloned()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn get(&mut self, provider: &str) -> Result<Zeroizing<String>, String> {
        self.load()?;
        self.values
            .as_ref()
            .and_then(|v| v.get(provider))
            .cloned()
            .ok_or("请先保存该供应商的 API Key。".into())
    }
}

#[cfg(target_os = "macos")]
fn read(service: &str) -> Result<HashMap<String, Zeroizing<String>>, String> {
    use security_framework::passwords::get_generic_password;
    let raw = match get_generic_password(service, "keys") {
        Ok(raw) => Zeroizing::new(raw),
        Err(e) if e.code() == -25300 => return Ok(HashMap::new()),
        Err(e) => {
            return Err(format!(
                "钥匙串暂不可读取（系统代码 {}），请授权后重试。",
                e.code()
            ))
        }
    };
    let decoded: HashMap<String, String> =
        serde_json::from_slice(&raw).map_err(|_| "钥匙串中的 Peck 数据无法读取。")?;
    Ok(decoded
        .into_iter()
        .map(|(k, v)| (k, Zeroizing::new(v)))
        .collect())
}

#[cfg(target_os = "macos")]
fn write(service: &str, values: &HashMap<String, Zeroizing<String>>) -> Result<(), String> {
    let plain: HashMap<&str, &str> = values
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let raw = Zeroizing::new(serde_json::to_vec(&plain).map_err(|_| "密钥无法编码。")?);
    security_framework::passwords::set_generic_password(service, "keys", &raw)
        .map_err(|e| format!("钥匙串保存失败（系统代码 {}）。", e.code()))
}

#[cfg(not(target_os = "macos"))]
fn read(_: &str) -> Result<HashMap<String, Zeroizing<String>>, String> {
    Err("真实密钥存储目前仅支持 macOS。".into())
}
#[cfg(not(target_os = "macos"))]
fn write(_: &str, _: &HashMap<String, Zeroizing<String>>) -> Result<(), String> {
    Err("真实密钥存储目前仅支持 macOS。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_keys_are_memory_only_and_status_has_no_secret() {
        let mut keys = Keys::new(None);
        keys.set("deepseek", Some("fake-only-for-unit-test".into()))
            .unwrap();
        assert_eq!(
            keys.get("deepseek").unwrap().as_str(),
            "fake-only-for-unit-test"
        );
        assert!(!serde_json::to_string(&keys.status())
            .unwrap()
            .contains("fake-only"));
        keys.set("deepseek", None).unwrap();
        assert!(keys.get("deepseek").is_err());
        assert!(keys.set("deepseek", Some("\n".into())).is_err());
    }
}
