pub mod budget;
pub mod keys;
pub mod profiles;
pub mod provider;
mod storage;
mod supervisor;
pub mod tools;
pub mod worker;

use crate::mode::ApiMode;
use crate::state::AppState;
use crate::store::{now_ms, Store};
use keys::Keys;
use profiles::{AgentProfile, Config, ModelProfile, Provider};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{ipc::Channel, State};
use tokio::sync::watch;

pub struct ApiState {
    pub(crate) mode: Mutex<ApiMode>,
    pub(crate) keys: Mutex<Keys>,
    pub(crate) active: Mutex<HashMap<String, watch::Sender<bool>>>,
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
            active: Mutex::new(HashMap::new()),
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
    pub response: Option<provider::Response>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub mode: ApiMode,
    pub providers: Vec<Provider>,
    pub config: Config,
    pub keys: Vec<keys::KeyStatus>,
    pub allowances: Vec<budget::Allowance>,
    pub calls: Vec<ApiCall>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type")]
pub enum ApiEvent {
    Text { id: String, text: String },
    Reasoning { id: String, text: String },
    Finished { call: ApiCall },
    PersistenceError { message: String },
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
    let (config, calls) = {
        let store = state.store.lock().map_err(|_| "无法读取 API 设置。")?;
        (
            store.api_config().map_err(|_| "无法读取 API 设置。")?,
            store.api_calls().map_err(|_| "无法读取 API 历史。")?,
        )
    };
    let path = crate::mode::budget_path(mode);
    let allowances = tokio::task::spawn_blocking(move || budget::read(&path))
        .await
        .map_err(|_| "无法读取预算。")??;
    Ok(Status {
        mode,
        providers: config.providers(),
        config,
        keys,
        allowances,
        calls,
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
    let active = state.api.active.lock().map_err(|_| "无法切换 API 模式。")?;
    if !active.is_empty() {
        return Err("请先停止当前 API 请求，再切换模式。".into());
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

#[tauri::command]
pub async fn api_request_start(
    id: String,
    role: String,
    prompt: String,
    scenario: String,
    channel: Channel<ApiEvent>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err("请求编号无效。".into());
    }
    if prompt.trim().is_empty() || prompt.len() > 80_000 {
        return Err("请输入任务内容，单次最多约两万汉字。".into());
    }
    if ![
        "success",
        "authentication",
        "rateLimit",
        "disconnect",
        "slow",
    ]
    .contains(&scenario.as_str())
    {
        return Err("未知模拟场景。".into());
    }
    let mut active = state.api.active.lock().map_err(|_| "无法创建 API 请求。")?;
    if !active.is_empty() {
        return Err("请先等待或停止当前 API 请求。".into());
    }
    let mode = *state.api.mode.lock().map_err(|_| "无法读取 API 模式。")?;
    if mode == ApiMode::Real && scenario != "success" {
        return Err("故障场景仅可在模拟模式使用。".into());
    }
    let (agent, model, provider) = state
        .store
        .lock()
        .map_err(|_| "无法读取模型设置。")?
        .api_config()
        .map_err(|_| "无法读取模型设置。")?
        .selection(&role)?;
    // Conservative bound for the single prompt in Phase 2; tool-context budgeting comes in Phase 3.
    if prompt.len() as u64 + 256 + agent.max_output_tokens as u64 > model.context_window as u64 {
        return Err("任务内容与输出预算超出模型上下文范围。".into());
    }
    let call = ApiCall {
        id: id.clone(),
        task_id: None,
        run_id: None,
        round: None,
        mode,
        role,
        model,
        settings: agent,
        prompt,
        started_at: now_ms(),
        finished_at: None,
        status: "running".into(),
        budget_counted: false,
        text: String::new(),
        reasoning: String::new(),
        response: None,
        error: None,
    };
    state
        .store
        .lock()
        .map_err(|_| "无法保存请求。")?
        .insert_api_call(&call)
        .map_err(|_| "无法创建调用记录；未发送请求。")?;
    let (tx, rx) = watch::channel(false);
    active.insert(id.clone(), tx);
    let api = state.api.clone();
    let store = state.store.clone();
    tokio::spawn(async move {
        execute(call, provider, scenario, api, store, rx, channel).await;
    });
    Ok(id)
}

#[tauri::command]
pub async fn api_request_cancel(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(tx) = state
        .api
        .active
        .lock()
        .map_err(|_| "无法停止 API 请求。")?
        .get(&id)
    {
        let _ = tx.send(true);
    }
    Ok(())
}

async fn execute(
    call: ApiCall,
    provider: Provider,
    scenario: String,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    cancel: watch::Receiver<bool>,
    channel: Channel<ApiEvent>,
) {
    let messages = vec![serde_json::json!({"role":"user","content":call.prompt})];
    let call = request_once(
        call,
        provider,
        messages,
        vec![],
        scenario,
        api.clone(),
        store,
        cancel,
        |event| {
            let _ = channel.send(event);
        },
    )
    .await;
    if let Ok(mut active) = api.active.lock() {
        active.remove(&call.id);
    }
    let _ = channel.send(ApiEvent::Finished { call });
}

// All sends (including subsequent tool rounds) use this same attempt-level budget entry.
pub(super) async fn request_once(
    mut call: ApiCall,
    provider: Provider,
    messages: Vec<serde_json::Value>,
    tools: Vec<serde_json::Value>,
    scenario: String,
    api: Arc<ApiState>,
    store: Arc<Mutex<Store>>,
    mut cancel: watch::Receiver<bool>,
    mut notify: impl FnMut(ApiEvent),
) -> ApiCall {
    let result = async {
        let key = if call.mode == ApiMode::Real {
            let api = api.clone();
            let provider_id = provider.id.clone();
            Some(
                tokio::task::spawn_blocking(move || {
                    api.keys
                        .lock()
                        .map_err(|_| "无法读取 API Key。".to_string())?
                        .get(&provider_id)
                })
                .await
                .map_err(|_| provider::Failure::Message("读取 API Key 失败。".into()))?
                .map_err(provider::Failure::Message)?,
            )
        } else {
            None
        };
        if *cancel.borrow() {
            return Err(provider::Failure::Cancelled);
        }
        let client = if call.mode == ApiMode::Real {
            Some(provider::client().map_err(provider::Failure::Message)?)
        } else {
            None
        };
        let path = crate::mode::budget_path(call.mode);
        let id = call.id.clone();
        let pid = provider.id.clone();
        let model = call.model.api_model.clone();
        tokio::task::spawn_blocking(move || budget::reserve(&path, &id, &pid, &model))
            .await
            .map_err(|_| provider::Failure::Message("预算保存任务中断，未发送请求。".into()))?
            .map_err(provider::Failure::Message)?;
        call.budget_counted = true;
        store
            .lock()
            .map_err(|_| provider::Failure::Message("调用记录不可用；未发送请求。".into()))?
            .update_api_call(&call)
            .map_err(|_| {
                provider::Failure::Message("调用记录保存失败；未发送请求，预算保守占用。".into())
            })?;
        let id = call.id.clone();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut checkpoint = call.clone();
        let mut saved_at = std::time::Instant::now();
        let mut emit = |delta| {
            match delta {
                provider::Delta::Text(t) => {
                    text.push_str(&t);
                    notify(ApiEvent::Text {
                        id: id.clone(),
                        text: t,
                    });
                }
                provider::Delta::Reasoning(t) => {
                    reasoning.push_str(&t);
                    notify(ApiEvent::Reasoning {
                        id: id.clone(),
                        text: t,
                    });
                }
            }
            if saved_at.elapsed() >= std::time::Duration::from_millis(500) {
                checkpoint.text = text.clone();
                checkpoint.reasoning = reasoning.clone();
                if store
                    .lock()
                    .map_err(|_| ())
                    .and_then(|db| db.update_api_call(&checkpoint).map_err(|_| ()))
                    .is_err()
                {
                    notify(ApiEvent::PersistenceError {
                        message: "流式检查点保存失败，请核对磁盘与历史。".into(),
                    });
                }
                saved_at = std::time::Instant::now();
            }
        };
        let result = if let (Some(client), Some(key)) = (&client, &key) {
            let body = provider::body(
                &call.model,
                &call.settings,
                &provider.protocol,
                messages,
                tools,
            );
            provider::stream(client, &provider, key, body, &mut cancel, &mut emit).await
        } else {
            if call.task_id.is_some()
                && scenario != "authentication"
                && scenario != "rateLimit"
                && scenario != "disconnect"
                && scenario != "slow"
            {
                worker::mock_response(
                    &provider.protocol,
                    &call.role,
                    call.round.unwrap_or(1),
                    &scenario,
                    &messages,
                    &mut cancel,
                    &mut emit,
                )
                .await
            } else {
                provider::mock_stream(provider.protocol.clone(), &scenario, &mut cancel, &mut emit)
                    .await
            }
        };
        call.text = text;
        call.reasoning = reasoning;
        result
    }
    .await;
    call.finished_at = Some(now_ms());
    match result {
        Ok(response) => {
            call.status = "completed".into();
            call.response = Some(response);
        }
        Err(provider::Failure::Cancelled) => call.status = "cancelled".into(),
        Err(provider::Failure::Message(e)) => {
            call.status = "failed".into();
            call.error = Some(e);
        }
    }
    if store
        .lock()
        .map_err(|_| ())
        .and_then(|s| s.update_api_call(&call).map_err(|_| ()))
        .is_err()
    {
        call.status = "failed".into();
        call.error = Some("最终调用历史保存失败，已停止后续工具与请求。".into());
        notify(ApiEvent::PersistenceError {
            message: "请求已结束，但最终历史写入失败；预算记录仍保留，请重开后核对。".into(),
        });
    }
    call
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_and_interrupted_requests_survive_restart_without_replay() {
        let path =
            std::env::temp_dir().join(format!("peck-api-history-{}.db", uuid::Uuid::new_v4()));
        let store = Store::open(&path).unwrap();
        let mut config = Config::default();
        config.agents[1].thinking = false;
        config.agents[1].effort = None;
        store.save_api_config(&config).unwrap();
        store.save_api_mode(ApiMode::Mock).unwrap();
        let (settings, model, _) = config.selection("worker").unwrap();
        let call = ApiCall {
            id: "interrupted".into(),
            task_id: None,
            run_id: None,
            round: None,
            mode: ApiMode::Mock,
            role: "worker".into(),
            model,
            settings,
            prompt: "实际任务".into(),
            started_at: 1,
            finished_at: None,
            status: "running".into(),
            budget_counted: true,
            text: "部分结果".into(),
            reasoning: String::new(),
            response: None,
            error: None,
        };
        store.insert_api_call(&call).unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        store.recover_api_calls().unwrap();
        assert!(!store.api_config().unwrap().agents[1].thinking);
        assert_eq!(store.api_mode().unwrap(), Some(ApiMode::Mock));
        let saved = store.api_calls().unwrap();
        assert_eq!(saved[0].status, "interrupted");
        assert!(saved[0].budget_counted);
        assert_eq!(saved[0].text, "部分结果");
        let mut invalid = config;
        invalid.agents[0].thinking = false;
        assert!(store.save_api_config(&invalid).is_err());
    }
}
