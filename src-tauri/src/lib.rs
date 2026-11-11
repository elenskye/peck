pub mod agent;
pub mod api;
pub mod commands;
pub mod events;
pub mod export;
pub mod files;
pub mod flow;
pub mod migrations;
pub mod mock;
pub mod mode;
pub mod process;
pub mod run;
pub mod snapshot;
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
            let monitor_arc = state.usage_monitor.clone();
            let api_arc = state.api.clone();
            app.manage(state);
            crate::usage::start_monitor(app.handle().clone(), store_arc, monitor_arc, api_arc);
            term::warm_env();
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Peck tauri application");
}
