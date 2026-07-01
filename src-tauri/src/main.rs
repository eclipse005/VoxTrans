#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use voxtrans::app_state;
use voxtrans::commands;
use voxtrans::db;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            // Tauri runs the setup closure on a worker thread of an existing
            // tokio runtime, so we must use the current Handle (with
            // block_in_place) instead of starting a fresh runtime.
            let pool = match tokio::runtime::Handle::try_current() {
                Ok(handle)
                    if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
                {
                    tokio::task::block_in_place(|| {
                        handle.block_on(db::init_pool(&app_handle))
                    })
                }
                _ => tauri::async_runtime::block_on(db::init_pool(&app_handle)),
            }
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(db::store::TaskStore::new(pool));
            app.manage(app_state::AppState {
                asr_model_download: Arc::new(std::sync::Mutex::new(
                    app_state::ModelDownloadRuntime::default(),
                )),
                align_model_download: Arc::new(std::sync::Mutex::new(
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
            commands::transcribe::separate_vocals,
            commands::file::save_srt,
            commands::file::export_srt,
            commands::file::export_task_srts,
            commands::transcription::build_source_sentences,
            commands::translate_terminology::build_terminology_layer,
            commands::translate_translation::build_translation_layer,
            commands::translate_connectivity::test_translate_llm,
            commands::file::get_file_size,
            commands::system::open_in_explorer,
            commands::system::open_output_dir,
            commands::system::open_task_output_dir,
            commands::system::open_task_log_dir,
            commands::system::list_system_fonts,
            commands::model::open_model_dir,
            commands::model::get_model_status,
            commands::model::start_model_download,
            commands::model::cancel_model_download,
            commands::preferences::load_user_preferences,
            commands::preferences::save_app_settings,
            commands::preferences::get_default_settings,
            commands::workspace::load_workspace_state,
            commands::workspace::load_workspace_task,
            commands::workspace::delete_tasks,
            commands::workspace::register_task_upload,
            commands::workspace::enqueue_task_run,
            commands::workspace::update_task_languages,
            commands::workspace::update_task_terminology,
            commands::workspace::save_subtitle_editor,
            commands::workspace::execute_task_run,
            commands::workspace::execute_task_batch,
            commands::workspace::enqueue_and_execute_task_batch,
            commands::logs::append_task_log,
            commands::logs::read_task_log,
            commands::logs::clear_task_logs,
            commands::logs::get_task_total_tokens,
            commands::updater::check_update,
            commands::updater::download_update,
            commands::updater::cancel_update,
            commands::updater::skip_update_version,
            commands::updater::get_skipped_version,
            commands::updater::open_external_url,
            commands::youtube::download_youtube_to_task_run,
            commands::youtube::get_youtube_download_progress,
            commands::youtube::list_youtube_download_progress,
            commands::youtube::cancel_youtube_download,
            commands::youtube::get_yt_dlp_version,
            commands::youtube::update_yt_dlp,
        ])
        .run(tauri::generate_context!())
        .expect("error while running voxtrans desktop");
}
