//! Tauri command entrypoints grouped by domain.
//!
//! Boundary:
//! - `transcribe`: ASR execution and low-level segmentation command.
//! - `transcription`: post-ASR pipeline orchestration and phase events.
pub mod file;
pub mod logs;
pub mod model;
pub mod preferences;
mod preferences_mapping;
pub mod preferences_types;
pub mod system;
pub mod transcribe;
mod transcribe_mapping;
pub mod transcribe_types;
pub mod transcription;
mod transcription_types;
pub mod translate;
pub mod translate_artifacts;
pub mod translate_connectivity;
pub mod translate_llm_settings;
pub mod translate_terminology;
pub mod translate_terms;
pub mod translate_translation;
pub mod translate_types;
pub mod updater;
pub mod workspace;
pub mod youtube;

#[cfg(test)]
mod command_registration_tests {
    const MAIN_RS: &str = include_str!("../main.rs");

    #[test]
    fn youtube_commands_are_registered_in_tauri_handler() {
        for command in [
            "commands::youtube::download_youtube_to_task_run",
            "commands::youtube::get_youtube_download_progress",
            "commands::youtube::list_youtube_download_progress",
            "commands::youtube::cancel_youtube_download",
            "commands::youtube::get_yt_dlp_version",
            "commands::youtube::update_yt_dlp",
        ] {
            assert!(
                MAIN_RS.contains(command),
                "{command} must be registered in the Tauri invoke handler"
            );
        }
    }

    #[test]
    fn final_check_command_is_not_registered_in_tauri_handler() {
        assert!(
            !MAIN_RS.contains("commands::translate_final_check::build_step_6_final_check"),
            "Step6 final check should not be exposed as a Tauri command"
        );
    }
}
