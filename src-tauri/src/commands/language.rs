use serde::Serialize;
use ts_rs::TS;

use crate::domain::language::LanguageTag;
use crate::domain::language_registry::LanguageRegistry;
use crate::services::preferences_types::{AlignModel, AsrModel};

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SourceLanguageOption {
    pub tag: LanguageTag,
    pub label: String,
    pub short: String,
}

#[tauri::command]
pub fn list_source_languages(
    asr_model: AsrModel,
    align_model: AlignModel,
) -> Result<Vec<SourceLanguageOption>, String> {
    Ok(LanguageRegistry::supported_for(asr_model, align_model)
        .into_iter()
        .map(|m| SourceLanguageOption {
            tag: m.tag,
            label: m.display_name.to_string(),
            short: m.short_badge.to_string(),
        })
        .collect())
}
