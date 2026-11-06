pub mod agent;
pub mod commands;
pub mod events;
pub mod flow;
pub mod mock;
pub mod mode;
pub mod process;
pub mod run;
pub mod snapshot;
pub mod state;
pub mod store;
pub mod tasks;
pub mod watch;
pub mod workspace;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let mut store = store::Store::open(&mode::app_data_dir().join("peck.db"))
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            store
                .recover_interrupted_runs()
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            app.manage(AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::agents_list,
            commands::agent_get_config,
            commands::agent_save_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Peck tauri application");
}
