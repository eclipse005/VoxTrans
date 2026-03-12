#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod db;
mod llm;
mod prompts;
mod services;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let pool = tauri::async_runtime::block_on(async { db::init_pool(&app_handle).await })?;
            app.manage(app_state::AppState {
                pool,
                model_download: Arc::new(std::sync::Mutex::new(
                    app_state::ModelDownloadRuntime::default(),
                )),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::transcribe::transcribe,
            commands::transcribe::build_segments_from_words,
            commands::file::save_srt,
            commands::subtitle::load_subtitle_editor,
            commands::subtitle::save_subtitle_editor,
            commands::file::get_file_size,
            commands::system::open_in_explorer,
            commands::system::open_output_dir,
            commands::model::open_model_dir,
            commands::model::get_model_status,
            commands::model::start_model_download,
            commands::model::cancel_model_download,
            commands::preferences::load_user_preferences,
            commands::preferences::save_app_settings,
            commands::preferences::save_terms,
            commands::preferences::save_hotword_correction,
            commands::workspace::load_workspace_state,
            commands::workspace::save_queue_state,
            commands::history::record_task_event,
            commands::history::list_task_events,
            commands::history::list_task_summaries,
            commands::history::clear_task_events,
            commands::history::delete_task_summaries,
            commands::logs::append_task_log,
            commands::logs::read_task_log,
            commands::logs::clear_task_logs,
            commands::usage::record_task_llm_usage,
            commands::usage::get_task_llm_usage_summary,
            prompts::build_hotword_correction_prompts,
            prompts::build_punctuation_restore_prompt,
            llm::llm_interact,
            llm::llm_test_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}
