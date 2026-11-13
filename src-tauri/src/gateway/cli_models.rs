//! CLI 引擎能选的模型（站主 2026-11-13 要的：在 AGENTS 里给 CLI 选模型）。
//!
//! 从 CLI 自己那里读，不起模型、不花额度：Codex `app-server` 的 `model/list`、Muse `serve` 的 `model/list`、
//! Grok / Antigravity（ACP）`session/new` 回应里的 `configOptions`（模型和强度）。Claude Code 没有列模型的接口，
//! 读它自己缓存的模型目录（配置目录下 `cache/model-catalog/*.json`：带版本的模型名、强度档、现在选的哪个）；
//! 没有缓存才退回 `--model` 认的别名（fable、opus、sonnet、haiku）。
//! 读到的存在 `app_settings` 的 `cli_models`，界面点 REFRESH 才重读；读不到照旧给上次的，带上原因。

use super::engines::{self, is_rpc_response, Launch, Line, Pipe};
use crate::state::AppState;
use crate::store::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliModel {
    /// 发给 CLI 的模型名。
    pub id: String,
    pub label: String,
    /// 这个模型能选的思考强度（CLI 的叫法）；空的是不能选。
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub default_effort: Option<String>,
    /// CLI 现在默认用的那个。
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliModels {
    pub engine: String,
    pub models: Vec<CliModel>,
    /// 从哪来：`cli`（问的 CLI 自己）、`builtin`（Peck 自带的别名）、`mock`（演示）。
    pub source: String,
    #[serde(default)]
    pub read_at: Option<i64>,
    /// 这一次没读到的原因（照旧给上次读到的）。
    #[serde(default)]
    pub error: Option<String>,
}

const CACHE_KEY: &str = "cli_models";
const CLAUDE_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Claude Code：`--model` 认的别名，强度是 `--effort` 的五档。
fn claude_code() -> Vec<CliModel> {
    [("fable", "Fable"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")]
        .into_iter()
        .map(|(id, label)| CliModel {
            id: id.into(),
            label: label.into(),
            efforts: CLAUDE_EFFORTS.iter().map(|s| s.to_string()).collect(),
            default_effort: None,
            is_default: false,
        })
        .collect()
}

/// Claude Code 缓存的模型目录：`catalog.config.models`（`id` 带版本，比如 `claude-opus-5-5`；`name` 是「Opus 5.5」；
/// `section` 是 main / overflow；`runtime.effort_levels`、`runtime.default_effort`），`catalog.state.model` 是现在选的。
pub fn parse_claude_catalog(v: &Value) -> Vec<CliModel> {
    let current = s(&v["catalog"]["state"]["model"]);
    let mut models: Vec<(bool, CliModel)> = v["catalog"]["config"]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = s(&m["id"])?;
            let mut efforts = efforts_of(&m["runtime"]["effort_levels"]);
            if efforts.is_empty() {
                efforts = efforts_of(&m["thinking"]["effort_options"]).into_iter().filter(|e| !e.is_empty()).collect();
                if efforts.is_empty() {
                    efforts = m["thinking"]["effort_options"].as_array().into_iter().flatten().filter_map(|o| s(&o["id"])).collect();
                }
            }
            let main = m["section"].as_str() != Some("overflow");
            Some((main, CliModel {
                label: s(&m["name"]).unwrap_or_else(|| id.clone()),
                efforts,
                default_effort: s(&m["runtime"]["default_effort"]),
                is_default: current.as_deref() == Some(id.as_str()),
                id,
            }))
        })
        .collect();
    // 常用的（main）在前，旧版本（overflow）在后，各自照目录里的顺序。
    models.sort_by_key(|(main, _)| !*main);
    models.into_iter().map(|(_, m)| m).collect()
}

/// 读 Claude Code 配置目录里最新的那份模型目录缓存；返回模型和它是什么时候取的。
fn claude_catalog(config_dir: &std::path::Path) -> Option<(Vec<CliModel>, Option<i64>)> {
    let newest = std::fs::read_dir(config_dir.join("cache").join("model-catalog"))
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
    let v: Value = serde_json::from_slice(&std::fs::read(newest.path()).ok()?).ok()?;
    let models = parse_claude_catalog(&v);
    (!models.is_empty()).then(|| (models, v["fetchedAt"].as_i64()))
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_string)
}

/// 强度的写法各家不同：字符串，或者 `{reasoningEffort | effort | value}`。
fn efforts_of(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| s(e).or_else(|| s(&e["reasoningEffort"])).or_else(|| s(&e["effort"])).or_else(|| s(&e["value"])))
        .collect()
}

/// Codex `model/list`：`{data: [{model | id, displayName, supportedReasoningEfforts, defaultReasoningEffort, isDefault, hidden}]}`。
pub fn parse_codex(result: &Value) -> Vec<CliModel> {
    let list = [&result["data"], &result["models"], &result["items"]].into_iter().find(|v| v.is_array()).cloned().unwrap_or(Value::Null);
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["hidden"] != json!(true))
        .filter_map(|m| {
            let id = s(&m["model"]).or_else(|| s(&m["id"]))?;
            Some(CliModel {
                label: s(&m["displayName"]).unwrap_or_else(|| id.clone()),
                efforts: efforts_of(&m["supportedReasoningEfforts"]),
                default_effort: s(&m["defaultReasoningEffort"]),
                is_default: m["isDefault"] == json!(true),
                id,
            })
        })
        .collect()
}

/// Muse `model/list`：`{models: [{modelId, displayLabel, variants, defaultReasoningEffort, isDefault}]}`。
pub fn parse_muse(result: &Value) -> Vec<CliModel> {
    result["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = s(&m["modelId"])?;
            let mut efforts = efforts_of(&m["variants"]);
            if efforts.is_empty() {
                efforts = efforts_of(&m["reasoningEffortVariants"]);
            }
            Some(CliModel {
                label: s(&m["displayLabel"]).unwrap_or_else(|| id.clone()),
                efforts,
                default_effort: s(&m["defaultReasoningEffort"]),
                is_default: m["isDefault"] == json!(true),
                id,
            })
        })
        .collect()
}

/// ACP 的 `session/new` 回应：`configOptions` 里 `model` 那一项的选项（可能分组），强度那一项给所有模型。
pub fn parse_acp(result: &Value, effort_option: &str) -> Vec<CliModel> {
    let options = result["configOptions"].as_array().cloned().unwrap_or_default();
    let find = |id: &str| options.iter().find(|o| o["id"] == id).cloned();
    let values = |o: &Value| -> Vec<(String, String)> {
        let mut out = Vec::new();
        for item in o["options"].as_array().into_iter().flatten() {
            if let Some(group) = item["options"].as_array() {
                for g in group {
                    if let Some(v) = s(&g["value"]) {
                        out.push((v.clone(), s(&g["name"]).unwrap_or(v)));
                    }
                }
            } else if let Some(v) = s(&item["value"]) {
                out.push((v.clone(), s(&item["name"]).unwrap_or(v)));
            }
        }
        out
    };
    let (efforts, default_effort) = match find(effort_option) {
        Some(o) => (values(&o).into_iter().map(|(v, _)| v).collect(), s(&o["currentValue"])),
        None => (Vec::new(), None),
    };
    let Some(model) = find("model") else { return Vec::new() };
    let current = s(&model["currentValue"]);
    values(&model)
        .into_iter()
        .map(|(id, label)| CliModel {
            is_default: current.as_deref() == Some(id.as_str()),
            efforts: efforts.clone(),
            default_effort: default_effort.clone(),
            label,
            id,
        })
        .collect()
}

/// 演示用（`?mock`、模拟模式）：照夹具里录到的写。
fn mock(engine: &str) -> Vec<CliModel> {
    let m = |id: &str, efforts: &[&str], is_default: bool| CliModel {
        id: id.into(),
        label: id.into(),
        efforts: efforts.iter().map(|s| s.to_string()).collect(),
        default_effort: None,
        is_default,
    };
    match engine {
        "claude-code" => {
            let five = ["low", "medium", "high", "xhigh", "max"];
            let mut list = vec![m("claude-opus-5-5", &five, true), m("claude-fable-5-2", &five, false), m("claude-sonnet-5-5", &five, false), m("claude-haiku-5-5", &five, false), m("claude-opus-4-8", &five, false)];
            for (x, name) in list.iter_mut().zip(["Opus 5.5", "Fable 5.2", "Sonnet 5.5", "Haiku 5.5", "Opus 4.8"]) {
                x.label = name.into();
            }
            list
        }
        "codex" => vec![m("gpt-6.1-sol", &["low", "medium", "high", "xhigh"], true), m("gpt-6-luna", &["low", "medium", "high"], false)],
        "muse" => vec![m("muse-wonder-1.0", &["low", "medium", "high", "max"], true), m("muse-ember-1.0", &["low", "medium", "high", "max"], false), m("muse-spark-1.3", &["low", "medium", "high", "max"], false)],
        "grok" => vec![m("grok-4.7", &["medium", "high", "xhigh"], true), m("grok-4.7-build-fast", &["medium", "high", "xhigh"], false), m("grok-4.6", &["medium", "high", "xhigh"], false)],
        "antigravity" => vec![m("gemini-4-argon", &["low", "medium", "high"], true), m("gemini-3.8-flash", &["low", "medium", "high"], false), m("claude-opus-5-5", &["low", "medium", "high"], false)],
        _ => Vec::new(),
    }
}

/// 发一个 JSON-RPC 请求、等它的回应；中间来的通知不管，CLI 反过来问的一律回「不支持」。
async fn rpc(pipe: &mut Pipe, id: u64, method: &str, params: Value) -> Result<Value, String> {
    pipe.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
        .await
        .map_err(|e| e.message)?;
    loop {
        match pipe.next().await {
            None => return Err(format!("CLI 在回 {method} 之前退出了。")),
            Some(Line::Json(v)) => {
                if is_rpc_response(&v) && v["id"] == json!(id) {
                    if let Some(m) = v["error"]["message"].as_str() {
                        return Err(format!("CLI 拒绝了 {method}：{}", super::convert::short(m, 160)));
                    }
                    return Ok(v["result"].clone());
                }
                if v.get("method").is_some() && v.get("id").is_some() {
                    let _ = pipe
                        .send(&json!({"jsonrpc": "2.0", "id": v["id"], "error": {"code": -32601, "message": "not supported"}}))
                        .await;
                }
            }
            Some(Line::Text(_)) => {}
        }
    }
}

fn client_info() -> Value {
    json!({"name": "peck", "title": "Peck", "version": env!("CARGO_PKG_VERSION")})
}

/// 起 CLI、问模型、关掉。
async fn ask(engine: &str, setup: engines::EngineSetup, cwd: std::path::PathBuf) -> Result<Vec<CliModel>, String> {
    let args: Vec<String> = match engine {
        "codex" => vec!["app-server".into()],
        "muse" => vec!["serve".into()],
        "grok" => engines::acp::GROK.args.iter().map(|s| s.to_string()).collect(),
        "antigravity" => engines::acp::ANTIGRAVITY.args.iter().map(|s| s.to_string()).collect(),
        _ => return Err("这家 CLI 不用读。".into()),
    };
    let launch = Launch { bin: setup.bin, args, cwd: cwd.clone(), env: setup.env, path: setup.path };
    let mut pipe = Pipe::spawn(&launch).map_err(|e| e.message)?;
    let result = match engine {
        "codex" => async {
            rpc(&mut pipe, 1, "initialize", json!({"clientInfo": client_info(), "capabilities": null})).await?;
            pipe.send(&json!({"jsonrpc": "2.0", "method": "initialized"})).await.map_err(|e| e.message)?;
            let mut all = Vec::new();
            let mut cursor: Option<String> = None;
            for page in 0..5u64 {
                let params = match &cursor {
                    Some(c) => json!({"cursor": c}),
                    None => json!({}),
                };
                let r = rpc(&mut pipe, 2 + page, "model/list", params).await?;
                all.extend(parse_codex(&r));
                cursor = s(&r["nextCursor"]);
                if cursor.is_none() {
                    break;
                }
            }
            Ok(all)
        }
        .await,
        "muse" => async {
            rpc(&mut pipe, 1, "initialize", json!({"clientInfo": client_info(), "capabilities": {"userInputDialogs": true}})).await?;
            pipe.send(&json!({"jsonrpc": "2.0", "method": "initialized"})).await.map_err(|e| e.message)?;
            let r = rpc(&mut pipe, 2, "model/list", json!({})).await?;
            Ok(parse_muse(&r))
        }
        .await,
        _ => async {
            let flavor = if engine == "grok" { engines::acp::GROK } else { engines::acp::ANTIGRAVITY };
            rpc(
                &mut pipe,
                1,
                "initialize",
                json!({"protocolVersion": 1,
                       "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                       "clientInfo": client_info()}),
            )
            .await?;
            let cwd = cwd.to_string_lossy().into_owned();
            let r = rpc(&mut pipe, 2, "session/new", json!({"cwd": cwd, "mcpServers": []})).await?;
            Ok(parse_acp(&r, flavor.effort_option))
        }
        .await,
    };
    pipe.stop(Duration::from_millis(500)).await;
    result
}

fn load_cache(state: &AppState) -> HashMap<String, CliModels> {
    state
        .store
        .lock()
        .ok()
        .and_then(|db| db.setting(CACHE_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_cache(state: &AppState, all: &HashMap<String, CliModels>) {
    if let (Ok(db), Ok(raw)) = (state.store.lock(), serde_json::to_string(all)) {
        let _ = db.set_setting(CACHE_KEY, &raw);
    }
}

/// 一家 CLI 能选的模型。`refresh` 为真才去问 CLI（第一次没存过也会问）。
#[tauri::command]
pub async fn cli_models(engine: String, refresh: bool, state: tauri::State<'_, AppState>) -> Result<CliModels, String> {
    let Some(spec) = super::catalog::catalog().engines.iter().find(|e| e.id == engine).cloned() else {
        return Err(format!("不认识的引擎 {engine}。"));
    };
    let is_mock = *state.api.mode.lock().map_err(|_| "读不到 API 模式。")? == crate::mode::ApiMode::Mock;
    if is_mock {
        return Ok(CliModels { engine: engine.clone(), models: mock(&engine), source: "mock".into(), read_at: Some(now_ms()), error: None });
    }
    if engine == "claude-code" {
        // 账号目录（Peck 里加的账号）或默认的 ~/.claude。
        let dir = {
            let db = state.store.lock().map_err(|_| "读不到设置。")?;
            let config = super::config::GatewayConfig::load(&db).map_err(|e| e.to_string())?;
            let book = super::accounts::AccountBook::load(&db);
            super::accounts::env_for(&book, &config, "claude-code")
                .into_iter()
                .find(|(k, _)| k == "CLAUDE_CONFIG_DIR")
                .map(|(_, v)| std::path::PathBuf::from(v))
                .unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".claude"))
        };
        return Ok(match claude_catalog(&dir) {
            Some((models, read_at)) => CliModels { engine, models, source: "cli".into(), read_at, error: None },
            None => CliModels {
                engine,
                models: claude_code(),
                source: "builtin".into(),
                read_at: None,
                error: Some("没找到 Claude Code 的模型目录（用过一次 Claude Code 才有），先给它认的几个别名。".into()),
            },
        });
    }
    let mut cache = load_cache(&state);
    let cached = cache.get(&engine).cloned();
    if !refresh {
        if let Some(c) = cached.as_ref().filter(|c| !c.models.is_empty()) {
            return Ok(CliModels { error: None, ..c.clone() });
        }
    }
    let (config, book) = {
        let db = state.store.lock().map_err(|_| "读不到设置。")?;
        (super::config::GatewayConfig::load(&db).map_err(|e| e.to_string())?, super::accounts::AccountBook::load(&db))
    };
    let fail = |error: String| CliModels {
        engine: engine.clone(),
        models: cached.as_ref().map(|c| c.models.clone()).unwrap_or_default(),
        source: "cli".into(),
        read_at: cached.as_ref().and_then(|c| c.read_at),
        error: Some(error),
    };
    let Some(mut setup) = engines::locate(&spec.command, &config).await else {
        return Ok(fail(format!("没找到 {}。", spec.label)));
    };
    setup.env = super::accounts::env_for(&book, &config, &engine);
    let cwd = crate::mode::app_data_dir().join("cli-probe");
    let _ = std::fs::create_dir_all(&cwd);
    match tokio::time::timeout(Duration::from_secs(25), ask(&engine, setup, cwd)).await {
        Ok(Ok(models)) if !models.is_empty() => {
            let fresh = CliModels { engine: engine.clone(), models, source: "cli".into(), read_at: Some(now_ms()), error: None };
            cache.insert(engine.clone(), fresh.clone());
            save_cache(&state, &cache);
            Ok(fresh)
        }
        Ok(Ok(_)) => Ok(fail(format!("{} 没列出模型。", spec.label))),
        Ok(Err(e)) => Ok(fail(e)),
        Err(_) => Ok(fail(format!("{} 25 秒没回。", spec.label))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_list_reads_ids_efforts_and_default() {
        let r = json!({"data": [
            {"id": "a", "model": "gpt-6.1-sol", "displayName": "GPT-6.1 Sol", "supportedReasoningEfforts": [{"reasoningEffort": "low"}, {"reasoningEffort": "high"}], "defaultReasoningEffort": "high", "isDefault": true},
            {"model": "old", "hidden": true},
            {"model": "gpt-6-luna", "supportedReasoningEfforts": ["low"]}
        ], "nextCursor": null});
        let m = parse_codex(&r);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].id.as_str(), m[0].label.as_str(), m[0].is_default), ("gpt-6.1-sol", "GPT-6.1 Sol", true));
        assert_eq!(m[0].efforts, vec!["low", "high"]);
        assert_eq!(m[0].default_effort.as_deref(), Some("high"));
        assert_eq!((m[1].label.as_str(), m[1].efforts.clone()), ("gpt-6-luna", vec!["low".to_string()]));
    }

    #[test]
    fn muse_list_from_the_recorded_fixture() {
        let raw = include_str!("../../tests/fixtures/gateway/muse-small-task/raw.jsonl");
        let reply = raw
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["id"] == json!(2) && v["result"]["models"].is_array())
            .expect("录音里有 model/list 的回应");
        let m = parse_muse(&reply["result"]);
        assert!(m.len() >= 3);
        assert_eq!(m[0].id, "muse-wonder-1.0");
        assert!(m[0].is_default && m[0].efforts.contains(&"high".to_string()));
    }

    #[test]
    fn acp_options_from_the_recorded_fixtures() {
        for (dir, option, first) in [("grok-small-task", "reasoning_effort", "grok-4.7"), ("antigravity-small-task", "thought_level", "gemini-4-argon")] {
            let raw = std::fs::read_to_string(format!("{}/tests/fixtures/gateway/{dir}/raw.jsonl", env!("CARGO_MANIFEST_DIR"))).unwrap();
            let reply = raw
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .find(|v| v["result"]["configOptions"].is_array())
                .expect("录音里有 session/new 的回应");
            let m = parse_acp(&reply["result"], option);
            assert_eq!(m.len(), 3, "{dir}");
            assert_eq!(m[0].id, first);
            assert!(m[0].is_default && !m[0].efforts.is_empty(), "{dir}");
        }
        // 分组的选项也认。
        let grouped = json!({"configOptions": [{"id": "model", "currentValue": "b", "options": [{"group": "g", "name": "G", "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]}]}]});
        let m = parse_acp(&grouped, "x");
        assert_eq!(m.iter().map(|x| (x.id.as_str(), x.is_default)).collect::<Vec<_>>(), vec![("a", false), ("b", true)]);
    }

    /// 真问一家 CLI 的模型（只握手、列模型，不发任务、不花额度）：
    /// `PECK_LIVE_CLI_DIR=…/cli/bin PECK_LIVE_ENGINE=codex cargo test live_list_models -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_list_models() {
        let engine = std::env::var("PECK_LIVE_ENGINE").unwrap_or_else(|_| "codex".into());
        let config = crate::gateway::config::GatewayConfig { cli_dir: std::env::var("PECK_LIVE_CLI_DIR").unwrap_or_default(), ..Default::default() };
        let spec = crate::gateway::catalog::catalog().engines.iter().find(|e| e.id == engine).cloned().unwrap();
        let setup = engines::locate(&spec.command, &config).await.expect("找得到 CLI");
        let cwd = std::env::temp_dir().join("peck-cli-probe");
        std::fs::create_dir_all(&cwd).unwrap();
        let models = tokio::time::timeout(Duration::from_secs(25), ask(&engine, setup, cwd)).await.expect("25 秒内回").expect("读得到");
        for m in &models {
            println!("{engine}: {} ({}) efforts={:?} default={:?} isDefault={}", m.id, m.label, m.efforts, m.default_effort, m.is_default);
        }
        assert!(!models.is_empty());
    }

    #[test]
    fn claude_catalog_lists_versioned_models_main_first() {
        let v = json!({"version": 2, "fetchedAt": 5, "catalog": {
            "config": {"models": [
                {"id": "claude-opus-5-5", "name": "Opus 5.5", "section": "main", "runtime": {"effort_levels": ["low", "medium", "high", "xhigh", "max"], "default_effort": "medium"}},
                {"id": "claude-opus-4-8", "name": "Opus 4.8", "section": "overflow", "runtime": {"effort_levels": ["low", "high"]}},
                {"id": "claude-haiku-4-5-20251001", "name": "Haiku 4.5", "section": "overflow", "thinking": {"type": "none"}, "runtime": {}},
                {"id": "claude-sonnet-5-5", "name": "Sonnet 5.5", "section": "main", "runtime": {"effort_levels": ["low"]}}
            ]},
            "state": {"model": "claude-opus-5-5"}
        }});
        let m = parse_claude_catalog(&v);
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["claude-opus-5-5", "claude-sonnet-5-5", "claude-opus-4-8", "claude-haiku-4-5-20251001"]);
        assert_eq!((m[0].label.as_str(), m[0].is_default, m[0].default_effort.as_deref()), ("Opus 5.5", true, Some("medium")));
        assert!(m[3].efforts.is_empty(), "不能思考的模型不给强度");
    }

    #[test]
    fn claude_code_aliases_carry_the_five_efforts() {
        let m = claude_code();
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["fable", "opus", "sonnet", "haiku"]);
        assert!(m.iter().all(|x| x.efforts.len() == 5));
    }
}
