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
pub mod transcription_cli;
pub mod transcription_grouping;
#[cfg(test)]
mod transcription_tests;
mod transcription_types;
pub mod translate;
pub mod translate_artifacts;
pub mod translate_cli;
pub mod translate_cli_args;
pub mod translate_cli_step5;
pub mod translate_cli_terminology;
pub mod translate_cli_translation;
pub mod translate_connectivity;
pub mod translate_defaults;
pub mod translate_final_check;
pub mod translate_llm_settings;
pub mod translate_quality;
pub mod translate_step5_commands;
pub mod translate_step5_common;
pub mod translate_step5_mapping;
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
}
