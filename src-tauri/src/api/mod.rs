//! 旧的 API 层（v0.2 起）剩下的：Key（钥匙串，Gateway 也用）、模拟 / 真实模式、百炼地域这类设置、
//! 旧的调用和规划 + 实施记录（只读，QUOTA 页和旧任务的历史要看）。
//! v0.8 Phase 11 删掉了旧的请求层、规划 + 实施（并进 Peck 循环的委派和合作）、开发者选项里的单次请求。
pub mod keys;
pub mod profiles;
mod storage;

use crate::mode::ApiMode;
use crate::state::AppState;
use keys::Keys;
use profiles::{AgentProfile, Config, ModelProfile, Provider};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

pub struct ApiState {
    pub(crate) mode: Mutex<ApiMode>,
    pub(crate) keys: Mutex<Keys>,
}
impl ApiState {
    pub fn new(mode: ApiMode) -> Self {
        Self {
            mode: Mutex::new(mode),
            keys: Mutex::new(Keys::new(if mode == ApiMode::Real {
                Some(crate::mode::keychain_service().into())
            } else {
                None
            })),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCall {
    pub id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub round: Option<u32>,
    pub mode: ApiMode,
    pub role: String,
    pub model: ModelProfile,
    pub settings: AgentProfile,
    pub prompt: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub budget_counted: bool,
    pub text: String,
    pub reasoning: String,
    /// 原来是旧请求层的整条回应；现在只读，留着原样。
    #[serde(default)]
    pub response: Option<serde_json::Value>,
    pub error: Option<String>,
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
        providers: config.providers(),
        config,
        keys,
    })
}

#[tauri::command]
pub async fn api_save_config(config: Config, state: State<'_, AppState>) -> Result<(), String> {
    config.validate()?;
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
        *state.api.keys.lock().map_err(|_| "无法切换密钥存储。")? =
            Keys::new(if mode == ApiMode::Real {
                Some(crate::mode::keychain_service().into())
            } else {
                None
            });
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

/// 旧的规划 + 实施任务的调用和审查历史（只读，原样给界面）。
#[tauri::command]
pub async fn api_task_history(run_id: String, state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let store = state.store.lock().map_err(|_| "读不到历史。")?;
    store.api_tasks(&run_id).map_err(|e| e.to_string())
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
    fn config_mode_and_old_records_survive_restart() {
        let path = std::env::temp_dir().join(format!("peck-api-history-{}.db", uuid::Uuid::new_v4()));
        let store = Store::open(&path).unwrap();
        let config = Config::default();
        store.save_api_config(&config).unwrap();
        store.save_api_mode(ApiMode::Mock).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO api_calls(id,task_id,started_at,record) VALUES('c1',NULL,1,?1)",
                [serde_json::json!({"id":"c1","mode":"mock","role":"worker","model":config.models[0],"settings":config.agents[0],"prompt":"x","startedAt":1,"finishedAt":null,"status":"running","budgetCounted":true,"text":"部分","reasoning":"","response":null,"error":null}).to_string()],
            )
            .unwrap();
        store.conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        store.conn.execute("INSERT INTO api_tasks(id,run_id,parent_id,role,status,record) VALUES('t1','run-1',NULL,'worker','running',?1)", [r#"{"id":"t1","status":"running","tools":[{"status":"running"}]}"#]).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        store.recover_api_calls().unwrap();
        store.recover_worker_tasks().unwrap();
        assert_eq!(store.api_mode().unwrap(), Some(ApiMode::Mock));
        let saved = store.api_calls().unwrap();
        assert_eq!(saved[0].status, "interrupted");
        assert_eq!(saved[0].text, "部分");
        let tasks = store.api_tasks("run-1").unwrap();
        assert_eq!(tasks[0]["status"], "interrupted");
        assert_eq!(tasks[0]["tools"][0]["status"], "interrupted");
        assert!(store.export_api_calls().unwrap()["calls"].as_array().unwrap().len() == 1);
    }
}
