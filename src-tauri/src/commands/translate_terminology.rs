use super::translate_llm_settings::hydrate_translate_llm_connection_settings;
use super::translate_terms::{
    count_source_tokens, load_terminology_entries_from_saved_settings,
    normalize_command_terminology_entries,
};
use super::translate_types::{
    BuildTerminologyLayerCommandRequest, BuildTerminologyLayerCommandResponse,
    TranslateTerminologyEntryCommand,
};
use crate::db::store::TaskStore;
use tauri::{AppHandle, Manager};

pub async fn build_terminology_layer(
    app: AppHandle,
    mut request: BuildTerminologyLayerCommandRequest,
) -> Result<BuildTerminologyLayerCommandResponse, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if request.target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    if request.segments.is_empty() {
        return Err("segments is required".to_string());
    }

    let store = app.state::<TaskStore>().inner();
    hydrate_translate_llm_connection_settings(
        store,
        &mut request.translate_api_key,
        &mut request.translate_base_url,
        &mut request.translate_model,
    )?;
    request.llm_concurrency = request.llm_concurrency.max(1);

    let terms = if request.terminology_entries.is_empty() {
        load_terminology_entries_from_saved_settings(store)?
    } else {
        normalize_command_terminology_entries(request.terminology_entries)
    };

    let source_token_total = count_source_tokens(&request.segments);
    if source_token_total == 0 {
        return Err("segments contain no valid text".to_string());
    }

    let service_request = crate::services::terminology::BuildTerminologyLayerRequest {
        task_id: request.task_id.clone(),
        media_path: request.media_path.clone(),
        source_lang: request.source_lang.clone(),
        target_lang: request.target_lang.clone(),
        segments: request
            .segments
            .iter()
            .map(|segment| crate::services::terminology::TerminologySegment {
                segment: segment.segment.clone(),
                tokens: segment
                    .tokens
                    .iter()
                    .map(|token| crate::services::terminology::TerminologyToken {
                        text: token.text.clone(),
                    })
                    .collect(),
            })
            .collect(),
        terminology_entries: terms
            .iter()
            .map(|entry| crate::services::terminology::TerminologyEntry {
                source: entry.source.clone(),
                target: entry.target.clone(),
                note: entry.note.clone(),
            })
            .collect(),
        translate_api_key: request.translate_api_key,
        translate_base_url: request.translate_base_url,
        translate_model: request.translate_model,
    };

    let service_response =
        crate::services::terminology::build_terminology_layer(service_request).await?;

    Ok(BuildTerminologyLayerCommandResponse {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        source_segment_total: request.segments.len(),
        source_token_total,
        theme_summary: service_response.theme_summary,
        terminology_entries: service_response
            .terminology_entries
            .into_iter()
            .map(|entry| TranslateTerminologyEntryCommand {
                source: entry.source,
                target: entry.target,
                note: entry.note,
            })
            .collect(),
    })
}
