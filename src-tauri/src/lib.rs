mod agent;
mod app_icon;
mod api;
mod commands;
mod eval;
mod events;
mod files;
mod flow;
mod gateway;
mod harness;
mod migrations;
mod mode;
mod plugins;
mod process;
mod project_plugins;
mod projects;
mod prompts;
mod run;
mod schedule;
mod skill_propose;
mod skills;
mod snapshot;
mod spend;
mod state;
mod store;
mod term;
mod tray;
mod usage;
mod watch;
mod workspace;
mod worktree;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 评测（v1.3.0）：`peck eval <attempt.json>` 不起窗口跑一次就退出；不带参数的照常起 App。
    if let Some(code) = eval::from_args() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(tray::TrayState::default())
        .setup(|app| {
            let mut store = store::Store::open(&mode::app_data_dir().join("peck.db"))
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            store
                .recover_interrupted_runs()
                .map_err(|e| e as Box<dyn std::error::Error>)?;
            // 独立工作区：任务删了、搬回丢掉了还留在磁盘上的，删掉（v0.8）。
            worktree::prune_orphans(&store);
            // API 价格表：站主改过的、刷新来的、汇率（v0.10 Refine）。
            gateway::prices::load(&store);
            let state = AppState::new(store).map_err(|e| e as Box<dyn std::error::Error>)?;
            let store_arc = state.store.clone();
            let gateway_store = state.store.clone();
            let monitor_arc = state.usage_monitor.clone();
            let api_arc = state.api.clone();
            let keys_api = state.api.clone();
            harness::mcp_client::set_keys(state.api.clone());
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
            // 「CLI 目录」没设：在后台从站主 shell 的 PATH 里找一个存上，引擎才检测得到。
            std::thread::spawn(move || gateway::config::fill_cli_dir(&gateway_store));
            // 插件库（v1.1.0）：程序里带的比数据目录里的新，就写进去。几个小文件，窗口出来之前写完，
            // 免得界面那边读到写了一半的。
            plugins::seed_library();
            // 落盘的长输出：14 天没动过的任务文件夹删掉。
            std::thread::spawn(|| harness::run::prune_spills(&harness::run::spill_root(), std::time::Duration::from_secs(14 * 86_400)));
            // 定时任务：关着时错过的记一笔，然后每 30 秒看一眼到点的。
            schedule::start(app.handle().clone());
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
            commands::workspace_open,
            commands::workspace_choose,
            commands::session_create,
            commands::session_get_runs,
            commands::run_cancel,
            commands::changes_get_diff,
            usage::usage_clients,
            usage::usage_refresh,
            usage::usage_manual_snapshot,
            usage::usage_balance,
            usage::usage_balances,
            commands::projects_overview,
            commands::files_list,
            commands::files_read,
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
            gateway::prices::prices_get,
            gateway::prices::prices_set,
            gateway::prices::prices_refresh,
            gateway::gateway_events,
            gateway::accounts::cli_accounts_list,
            gateway::accounts::cli_account_check,
            gateway::accounts::cli_account_login,
            gateway::accounts::cli_account_login_state,
            gateway::accounts::cli_account_login_input,
            gateway::accounts::cli_account_login_cancel,
            gateway::runner::gateway_run_start,
            gateway::runner::gateway_compact,
            gateway::runner::gateway_control,
            harness::run::harness_run_start,
            harness::run::harness_oneshot,
            harness::mcp_client::mcp_servers_get,
            harness::mcp_client::mcp_servers_set,
            harness::mcp_client::mcp_server_test,
            harness::mcp_client::mcp_secret_set,
            harness::mcp_client::project_mcp_list,
            harness::mcp_client::project_mcp_test,
            plugins::plugin_list,
            plugins::plugin_every_project,
            project_plugins::project_prepare,
            project_plugins::project_plugins,
            project_plugins::plugin_install,
            project_plugins::plugin_remove,
            skills::slash_list,
            skills::slash_expand,
            skill_propose::skill_review,
            skill_propose::skill_overview,
            skill_propose::skill_versions,
            skill_propose::skill_restore,
            harness::run::harness_compact,
            harness::run::harness_output_read,
            harness::run::harness_image_read,
            harness::run::upload_read_image,
            harness::run::upload_stage,
            commands::open_path,
            commands::open_url,
            schedule::schedule_list,
            schedule::schedule_delete,
            schedule::schedule_missed,
            harness::run::harness_spend,
            harness::run::harness_allow_list,
            projects::workspace_update,
            projects::workspace_remove,
            projects::workspace_relocate,
            projects::workspace_reveal,
            projects::session_rename,
            projects::session_delete,
            projects::session_archive,
            projects::session_unarchive,
            projects::workspace_archive_tasks,
            projects::archive_list,
            projects::project_trust,
            projects::layout_save,
            spend::spend_overview,
            harness::run::harness_allow_remove,
            commands::dialog_pick_folder,
            commands::dialog_pick_uploads,
            commands::files_children,
            commands::files_quicklook,
            commands::files_quicklook_open,
            worktree::worktree_apply,
            worktree::worktree_discard,
            harness::history::memory_list,
            harness::history::memory_overview,
            harness::history::memory_add,
            harness::history::memory_update,
            harness::history::memory_delete,
            harness::history::memory_review,
            prompts::prompt_list,
            prompts::prompt_add,
            prompts::prompt_update,
            prompts::prompt_delete,
            harness::background::background_list,
            harness::background::background_stop,
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
            // 退出 Peck：后台命令和常驻的 dev server 都停掉（整个进程组）。
            if let tauri::RunEvent::Exit = event {
                harness::background::stop_all();
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            // 窗口藏起来以后点 Dock 里的图标，把窗口叫回来。
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
