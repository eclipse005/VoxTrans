#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod db;
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
                asr_model_download: Arc::new(std::sync::Mutex::new(
                    app_state::ModelDownloadRuntime::default(),
                )),
                demucs_model_download: Arc::new(std::sync::Mutex::new(
                    app_state::ModelDownloadRuntime::default(),
                )),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::transcribe::transcribe,
            commands::transcribe::build_segments_from_words,
            commands::transcribe::separate_vocals,
            commands::file::save_srt,
            commands::file::export_srt,
            commands::subtitle::load_subtitle_editor,
            commands::subtitle::save_subtitle_editor,
            commands::transcription::run_post_asr_pipeline,
            commands::translate::run_translate_pipeline,
            commands::translate::test_translate_llm,
            commands::file::get_file_size,
            commands::system::open_in_explorer,
            commands::system::open_output_dir,
            commands::system::open_task_output_dir,
            commands::system::open_task_log_dir,
            commands::model::open_model_dir,
            commands::model::get_model_status,
            commands::model::start_model_download,
            commands::model::cancel_model_download,
            commands::preferences::load_user_preferences,
            commands::preferences::save_app_settings,
            commands::workspace::load_workspace_state,
            commands::workspace::save_queue_state,
            commands::task_engine::register_task_upload,
            commands::task_engine::enqueue_task_run,
            commands::task_engine::list_task_runs,
            commands::task_engine::get_task_run,
            commands::task_engine::execute_task_run,
            commands::task_engine::execute_task_batch,
            commands::task_engine::enqueue_and_execute_task_batch,
            commands::history::record_task_event,
            commands::history::list_task_events,
            commands::history::list_task_summaries,
            commands::history::clear_task_events,
            commands::history::delete_task_summaries,
            commands::logs::append_task_log,
            commands::logs::read_task_log,
            commands::logs::clear_task_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}
