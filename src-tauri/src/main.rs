#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use voxtrans::app_state;
use voxtrans::commands;
use voxtrans::db;
use voxtrans::services::app_log;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    // First statements in the process: a release build sets
    // `windows_subsystem = "windows"`, so stderr goes nowhere and a bootstrap
    // failure or panic would leave no trace at all. `app_log` resolves its path
    // without an AppHandle precisely so it is available this early.
    app_log::init();
    app_log::install_panic_hook();
    // Native faults — a CUDA driver error, an illegal access inside a C++ engine
    // — reach neither a Rust `Result` nor the panic hook. This leaves a report
    // the next start promotes into the log.
    app_log::install_crash_handler();
    // Report what the previous run left behind (an unfinished session, a crash
    // report), then mark this session as started.
    app_log::session_start();

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
            };
            let pool = match pool {
                Ok(pool) => pool,
                Err(err) => {
                    app_log::error("app.startup", "DB_INIT_FAILED", &err, None);
                    return Err(err.into());
                }
            };
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
            commands::file::export_task_srts,
            commands::translate_connectivity::test_translate_llm,
            commands::translate_connectivity::list_llm_models,
            commands::file::get_file_size,
            commands::system::open_task_output_dir,
            commands::system::open_task_log_dir,
            commands::system::open_app_log_dir,
            commands::system::list_system_fonts,
            commands::model::open_model_dir,
            commands::model::get_model_status,
            commands::model::start_model_download,
            commands::model::cancel_model_download,
            commands::language::list_source_languages,
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
            commands::workspace::update_task_review_flags,
            commands::workspace::resume_task_after_review,
            commands::workspace::save_subtitle_editor,
            commands::workspace::execute_task_batch,
            commands::logs::read_task_log,
            commands::logs::report_frontend_error,
            commands::logs::clear_task_logs,
            commands::logs::get_task_total_tokens,
            commands::updater::check_update,
            commands::updater::download_update,
            commands::updater::cancel_update,
            commands::updater::skip_update_version,
            commands::updater::get_skipped_version,
            commands::updater::open_external_url,
            commands::youtube::download_youtube_to_task_run,
            commands::youtube::cancel_youtube_download,
            commands::youtube::get_yt_dlp_version,
            commands::youtube::update_yt_dlp,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| {
            app_log::error("app.startup", "APP_RUN_FAILED", err.to_string(), None);
            // Shutting down deliberately after recording the failure: still a
            // clean marker, so the next start reports the failure, not a
            // phantom "unexpected exit".
            app_log::session_stop();
            std::process::exit(1);
        })
        // `App::run` never returns: it dispatches `RunEvent::Exit` to this
        // callback and then calls `process::exit(0)` itself. Writing the stop
        // marker after it would never happen, and every normal quit would be
        // reported as an unexpected exit at the next start — hence the marker
        // lives in the callback.
        .run(|_handle, event| {
            if let tauri::RunEvent::Exit = event {
                app_log::session_stop();
            }
        });
}
