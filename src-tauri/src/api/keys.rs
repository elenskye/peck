use std::collections::HashMap;
use std::sync::Mutex;
use zeroize::Zeroizing;

// Ordinary settings and IPC status never contain credentials.
//
// 钥匙串里按「包」存（站主 2026-11-13 定）：AI 供应商的 Key 一包、其他的 Key（以后新加的都算）一包，
// 一包是一个钥匙串项。Peck 每次更新以后签名变了，系统每一包问一次授权，所以每次更新最多授权两次。
// 打开 App 就读两包（`load_unlocked`），授权窗口在那时弹出来，不用等到发任务或进设置。

/// Key 归哪一包。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyGroup {
    /// 各家 AI 供应商（`profiles::providers()`）。
    Ai,
    /// 不是 AI 的 Key：以后加的都放这一包。
    Other,
}

impl KeyGroup {
    const ALL: [KeyGroup; 2] = [KeyGroup::Ai, KeyGroup::Other];

    /// 钥匙串项的账户名（AI 这一包沿用 v0.2 以来的 `keys`，旧 Key 不用搬）。
    fn account(self) -> &'static str {
        match self {
            KeyGroup::Ai => "keys",
            KeyGroup::Other => "other-keys",
        }
    }
}

/// 不是 AI 的 Key：（id，名字）。现在还没有；加一个就是在这里加一行，它自动进「其他」那一包。
pub const OTHER_KEYS: &[(&str, &str)] = &[];

/// 这个 Key 归哪一包；不认识的是 None。
pub fn group_of(id: &str) -> Option<KeyGroup> {
    if super::profiles::providers().iter().any(|p| p.id == id) {
        Some(KeyGroup::Ai)
    } else if OTHER_KEYS.iter().any(|(k, _)| *k == id) {
        Some(KeyGroup::Other)
    } else {
        None
    }
}

fn all_ids() -> Vec<String> {
    super::profiles::providers()
        .into_iter()
        .map(|p| p.id)
        .chain(OTHER_KEYS.iter().map(|(k, _)| k.to_string()))
        .collect()
}

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
        all_ids()
            .into_iter()
            .map(|id| KeyStatus {
                state: if self.failed {
                    "unavailable"
                } else if let Some(v) = &self.values {
                    if v.contains_key(&id) {
                        "saved"
                    } else {
                        "missing"
                    }
                } else {
                    "unloaded"
                }
                .into(),
                provider_id: id,
            })
            .collect()
    }

    pub fn load(&mut self) -> Result<(), String> {
        if self.values.is_some() {
            return Ok(());
        }
        let result = read_all(self.service.as_deref());
        self.apply(result)
    }

    /// 读钥匙串（可能弹授权窗口）的时候不占着锁：设置页、模型列表照常能看状态。
    /// 已经读过、或者读的时候换了模式，就什么也不做；读到了返回 true。
    pub fn load_unlocked(keys: &Mutex<Keys>) -> Result<bool, String> {
        let service = {
            let k = keys.lock().map_err(|_| "无法读取密钥。")?;
            if k.values.is_some() {
                return Ok(false);
            }
            k.service.clone()
        };
        let result = read_all(service.as_deref());
        let mut k = keys.lock().map_err(|_| "无法读取密钥。")?;
        if k.values.is_some() || k.service != service {
            return Ok(false);
        }
        k.apply(result).map(|_| true)
    }

    fn apply(&mut self, result: Result<HashMap<String, Zeroizing<String>>, String>) -> Result<(), String> {
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
        let Some(group) = group_of(provider) else {
            return Err("未知供应商。".into());
        };
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
            // 只写这个 Key 所在的那一包。
            let bundle: HashMap<&str, &str> = values
                .iter()
                .filter(|(k, _)| group_of(k) == Some(group))
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            write(service, group, &bundle)?;
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

/// 两包都读、合在一起；没有这一项（-25300）是空的，不弹窗。
fn read_all(service: Option<&str>) -> Result<HashMap<String, Zeroizing<String>>, String> {
    let Some(service) = service else {
        return Ok(HashMap::new());
    };
    let mut all = HashMap::new();
    for group in KeyGroup::ALL {
        for (k, v) in read(service, group)? {
            // 只收归这一包的：别的包里混进来的（不该有）不认。
            if group_of(&k) == Some(group) {
                all.insert(k, v);
            }
        }
    }
    Ok(all)
}

#[cfg(target_os = "macos")]
fn read(service: &str, group: KeyGroup) -> Result<HashMap<String, Zeroizing<String>>, String> {
    use security_framework::passwords::get_generic_password;
    let raw = match get_generic_password(service, group.account()) {
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
fn write(service: &str, group: KeyGroup, values: &HashMap<&str, &str>) -> Result<(), String> {
    let raw = Zeroizing::new(serde_json::to_vec(values).map_err(|_| "密钥无法编码。")?);
    security_framework::passwords::set_generic_password(service, group.account(), &raw)
        .map_err(|e| format!("钥匙串保存失败（系统代码 {}）。", e.code()))
}

#[cfg(not(target_os = "macos"))]
fn read(_: &str, _: KeyGroup) -> Result<HashMap<String, Zeroizing<String>>, String> {
    Err("真实密钥存储目前仅支持 macOS。".into())
}
#[cfg(not(target_os = "macos"))]
fn write(_: &str, _: KeyGroup, _: &HashMap<&str, &str>) -> Result<(), String> {
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

    #[test]
    fn ai_keys_share_one_bundle_and_unknown_ids_are_refused() {
        // 各家 AI 供应商都归 AI 那一包（一个钥匙串项、一次授权）。
        for p in crate::api::profiles::providers() {
            assert_eq!(group_of(&p.id), Some(KeyGroup::Ai));
        }
        assert_eq!(KeyGroup::Ai.account(), "keys");
        assert_ne!(KeyGroup::Other.account(), KeyGroup::Ai.account());
        assert_eq!(group_of("no-such-key"), None);
        let mut keys = Keys::new(None);
        assert!(keys.set("no-such-key", Some("x".into())).is_err());
    }

    #[test]
    fn load_unlocked_reads_once_and_marks_loaded() {
        let keys = Mutex::new(Keys {
            service: None,
            values: None,
            failed: false,
            revision: 0,
        });
        assert!(Keys::load_unlocked(&keys).unwrap());
        assert!(!Keys::load_unlocked(&keys).unwrap());
        assert!(keys.lock().unwrap().status().iter().all(|k| k.state == "missing"));
    }
}
