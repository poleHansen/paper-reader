mod commands;
mod errors;
mod models;
mod providers;
mod repositories;
mod services;
mod state;
mod utils;

use std::sync::Arc;

use repositories::database::Database;
use services::{library_service::LibraryService, model_service::ModelService, paper_service::PaperService, profile_service::ProfileService, runtime_service::RuntimeService};
use state::AppState;
use tauri::Manager;

pub fn run() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let database = Arc::new(Database::new(&handle)?);
            let state = AppState {
                profile_service: Arc::new(ProfileService::new(database.clone())),
                model_service: Arc::new(ModelService::new(database.clone())),
                paper_service: Arc::new(PaperService::new(database.clone())),
                library_service: Arc::new(LibraryService::new(database.clone())),
                runtime_service: Arc::new(RuntimeService::new(database.clone())),
            };
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::agent_commands::run_agent,
            commands::agent_commands::get_agent_run,
            commands::profile_commands::get_profile,
            commands::profile_commands::upsert_profile,
            commands::model_commands::save_model_config,
            commands::model_commands::update_model_config,
            commands::model_commands::list_model_configs,
            commands::model_commands::get_model_config_detail,
            commands::model_commands::get_recent_model_config,
            commands::model_commands::select_model_config,
            commands::model_commands::delete_model_config,
            commands::model_commands::test_model_connection,
            commands::paper_commands::search_papers,
            commands::paper_commands::import_paper_from_file,
            commands::paper_commands::import_paper_from_link,
            commands::paper_commands::pick_pdf_file,
            commands::paper_commands::confirm_paper_metadata,
            commands::paper_commands::get_paper_parse_status,
            commands::paper_commands::get_reader_snapshot,
            commands::library_commands::save_to_library,
            commands::library_commands::update_library_item,
            commands::library_commands::list_library_items,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run paper-reader");
}
