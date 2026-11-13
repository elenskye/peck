pub mod agent;
pub mod app_icon;
pub mod api;
pub mod commands;
pub mod events;
pub mod export;
pub mod files;
pub mod flow;
pub mod gateway;
pub mod harness;
pub mod migrations;
pub mod mock;
pub mod mode;
pub mod process;
pub mod projects;
pub mod run;
pub mod snapshot;
pub mod spend;
pub mod state;
pub mod store;
pub mod tasks;
pub mod term;
pub mod tray;
pub mod usage;
pub mod watch;
pub mod workspace;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(tray::TrayState::default())
        .setup(|app| {
            let mut store = store::Store::open(&mode::app_data_dir().join("peck.db"))
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            store
                .recover_interrupted_runs()
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            store
                .recover_api_calls()
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            store
                .recover_worker_tasks()
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            let state = AppState::new(store).map_err(|e| e as Box<dyn std::error::Error>)?;
            let store_arc = state.store.clone();
            let gateway_store = state.store.clone();
            let monitor_arc = state.usage_monitor.clone();
            let api_arc = state.api.clone();
            let keys_api = state.api.clone();
            app.manage(state);
            // 打开 App 就读钥匙串里的 Key（AI 一包、其他一包）：更新以后系统在这时弹授权窗口，
            // 不用等到发任务或进设置；读完告诉界面重读模型列表和 Key 状态。
            let keys_app = app.handle().clone();
            std::thread::spawn(move || {
                use tauri::Emitter;
                if !matches!(api::keys::Keys::load_unlocked(&keys_api.keys), Ok(false)) {
                    let _ = keys_app.emit("keys-loaded", ());
                }
            });
            crate::usage::start_monitor(app.handle().clone(), store_arc, monitor_arc, api_arc);
            term::warm_env();
            // 「CLI 目录」没设：在后台找一个存上（旧清单旁边的 bin/、站主 shell 的 PATH），引擎才检测得到。
            gateway::config::fill_cli_dir(&gateway_store, false);
            std::thread::spawn(move || gateway::config::fill_cli_dir(&gateway_store, true));
            // 落盘的长输出：14 天没动过的任务文件夹删掉。
            std::thread::spawn(|| harness::run::prune_spills(&harness::run::spill_root(), std::time::Duration::from_secs(14 * 86_400)));
            // 菜单栏图标起不来不挡 App 打开。
            if let Err(error) = tray::init(app) {
                eprintln!("Peck 菜单栏图标没起来: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api::api_status,
            api::api_save_config,
            api::api_set_mode,
            api::api_key_load,
            api::api_key_set,
            api::api_request_start,
            api::api_request_cancel,
            api::worker::api_run_start,
            api::worker::api_task_history,
            api::worker::api_export_calls,
            commands::workspace_open,
            commands::workspace_choose,
            commands::workspace_list_recent,
            commands::session_create,
            commands::session_list,
            commands::session_get_messages,
            commands::session_get_runs,
            commands::run_dispatch,
            commands::run_cancel,
            commands::run_retry_step,
            commands::changes_get_diff,
            usage::usage_list,
            usage::usage_summary,
            usage::usage_import,
            usage::usage_export_csv,
            usage::usage_clients,
            usage::usage_refresh,
            usage::usage_manual_snapshot,
            usage::usage_balance,
            usage::usage_balances,
            commands::agents_list,
            commands::agent_get_config,
            commands::agent_save_config,
            commands::projects_overview,
            commands::files_list,
            commands::files_read,
            commands::run_undo_preview,
            commands::run_undo,
            commands::settings_get,
            commands::settings_set,
            commands::term_run,
            commands::term_stop,
            commands::term_state,
            commands::shell_set_activity,
            commands::notify,
            gateway::gateway_models,
            gateway::cli_models::cli_models,
            gateway::gateway_config_get,
            gateway::gateway_config_set,
            gateway::gateway_events,
            gateway::accounts::cli_accounts_list,
            gateway::accounts::cli_account_check,
            gateway::accounts::cli_account_login,
            gateway::accounts::cli_account_login_state,
            gateway::accounts::cli_account_login_input,
            gateway::accounts::cli_account_login_cancel,
            gateway::runner::gateway_run_start,
            gateway::runner::gateway_control,
            harness::run::harness_run_start,
            harness::run::harness_oneshot,
            harness::run::harness_output_read,
            harness::run::harness_spend,
            harness::run::harness_allow_list,
            projects::workspace_update,
            projects::workspace_remove,
            projects::workspace_relocate,
            projects::workspace_reveal,
            projects::session_rename,
            projects::session_delete,
            projects::layout_save,
            spend::spend_overview,
            harness::run::harness_allow_remove,
            app_icon::app_icon_set,
        ])
        // 点红色关闭只把窗口藏起来：菜单栏图标和在跑的任务都留着；⌘Q 或菜单栏图标的「退出 Peck」才退出。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Peck tauri application")
        .run(|app, event| {
            // 窗口藏起来以后点 Dock 里的图标，把窗口叫回来。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
