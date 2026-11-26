//! Key（钥匙串，Gateway 也用）、模拟 / 真实模式、Aliyun 的地域。
//! 旧的 API 层（v0.2–v0.7 的请求层、规划 + 实施）和它的调用记录 v1.0.3 起不再读（库里的行留着）。
pub mod keys;
pub mod profiles;
mod storage;

use crate::mode::ApiMode;
use crate::state::AppState;
use keys::Keys;
use profiles::{Config, Provider};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

pub struct ApiState {
    pub(crate) mode: Mutex<ApiMode>,
    pub(crate) keys: Mutex<Keys>,
}
/// 真实模式用钥匙串，模拟模式只在内存。
fn keys_for(mode: ApiMode) -> Keys {
    Keys::new((mode == ApiMode::Real).then(|| crate::mode::keychain_service().into()))
}

impl ApiState {
    pub fn new(mode: ApiMode) -> Self {
        Self {
            mode: Mutex::new(mode),
            keys: Mutex::new(keys_for(mode)),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub mode: ApiMode,
    pub providers: Vec<Provider>,
    pub config: Config,
    pub keys: Vec<keys::KeyStatus>,
}

#[tauri::command]
pub async fn api_status(state: State<'_, AppState>) -> Result<Status, String> {
    let mode = *state.api.mode.lock().map_err(|_| "无法读取 API 模式。")?;
    let keys = state
        .api
        .keys
        .lock()
        .map_err(|_| "无法读取密钥状态。")?
        .status();
    let config = {
        let store = state.store.lock().map_err(|_| "无法读取 API 设置。")?;
        store.api_config().map_err(|_| "无法读取 API 设置。")?
    };
    Ok(Status {
        mode,
        providers: profiles::providers(),
        config,
        keys,
    })
}

#[tauri::command]
pub async fn api_save_config(config: Config, state: State<'_, AppState>) -> Result<(), String> {
    state
        .store
        .lock()
        .map_err(|_| "无法保存 API 设置。")?
        .save_api_config(&config)
        .map_err(|_| "无法保存 API 设置。".into())
}

#[tauri::command]
pub async fn api_set_mode(mode: ApiMode, state: State<'_, AppState>) -> Result<(), String> {
    if state.active_runs.lock().map(|a| !a.is_empty()).unwrap_or(true) {
        return Err("有任务在跑，先停下再切换模式。".into());
    }
    let mut current = state.api.mode.lock().map_err(|_| "无法切换 API 模式。")?;
    if *current != mode {
        state
            .store
            .lock()
            .map_err(|_| "无法保存 API 模式。")?
            .save_api_mode(mode)
            .map_err(|_| "无法保存 API 模式。")?;
        *state.api.keys.lock().map_err(|_| "无法切换密钥存储。")? = keys_for(mode);
        *current = mode;
    }
    Ok(())
}

#[tauri::command]
pub async fn api_key_load(state: State<'_, AppState>) -> Result<(), String> {
    let api = state.api.clone();
    tokio::task::spawn_blocking(move || Keys::load_unlocked(&api.keys).map(|_| ()))
        .await
        .map_err(|_| "读取密钥任务中断。")?
}

#[tauri::command]
pub async fn api_key_set(
    provider_id: String,
    key: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let api = state.api.clone();
    let changed_provider = provider_id.clone();
    tokio::task::spawn_blocking(move || {
        api.keys
            .lock()
            .map_err(|_| "无法保存密钥。")?
            .set(&provider_id, key)
    })
    .await
    .map_err(|_| "保存密钥任务中断。")??;
    state
        .store
        .lock()
        .map_err(|_| "无法更新余额状态。")?
        .usage_clear_balance(&changed_provider)
        .map_err(|e| e.to_string())?;
    state
        .usage_monitor
        .last_query
        .lock()
        .map_err(|_| "无法更新余额状态。")?
        .remove(&format!("balance/{changed_provider}"));
    Ok(())
}

/// 发 HTTP 的客户端（QUOTA 页查余额用）：不跟重定向、不自动重试。
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(std::time::Duration::from_secs(20))
        .read_timeout(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|_| "无法创建 API 连接。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn config_and_mode_survive_restart() {
        let path = std::env::temp_dir().join(format!("peck-api-settings-{}.db", uuid::Uuid::new_v4()));
        let store = Store::open(&path).unwrap();
        store.save_api_config(&Config { aliyun_region: profiles::AliyunRegion::Singapore }).unwrap();
        store.save_api_mode(ApiMode::Mock).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.api_mode().unwrap(), Some(ApiMode::Mock));
        assert!(matches!(store.api_config().unwrap().aliyun_region, profiles::AliyunRegion::Singapore));
    }
}
