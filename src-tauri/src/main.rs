#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod commands;
mod domain;
mod services;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    commands::transcription::maybe_run_build_source_sentences_mode_from_args();
    commands::translate::maybe_run_build_terminology_mode_from_args();
    commands::translate::maybe_run_build_translation_mode_from_args();
    commands::translate::maybe_run_build_step5_mode_from_args();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
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
            commands::translate_step5_commands::build_step_5_1_source_split,
            commands::translate_step5_commands::build_step_5_2_translation_align,
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
            commands::workspace::load_workspace_state,
            commands::workspace::load_workspace_task,
            commands::workspace::delete_tasks,
            commands::workspace::register_task_upload,
            commands::workspace::enqueue_task_run,
            commands::workspace::update_task_languages,
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
