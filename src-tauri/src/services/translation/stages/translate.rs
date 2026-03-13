use crate::services::translation::domain::{SentenceUnit, StageResult, TranslatedUnit, TranslationProfile};
use crate::services::translation::llm::TranslationLlmClient;

pub async fn run_stage(
    llm: &TranslationLlmClient,
    source_language: &str,
    target_language: &str,
    summary: &TranslationProfile,
    units: &[SentenceUnit],
) -> Result<StageResult<Vec<TranslatedUnit>>, String> {
    let units = llm.translate_sentences(
        source_language,
        target_language,
        Some(summary.translation_style.as_str()),
        Some(summary.topic_summary.as_str()),
        &summary.primary_terms,
        &summary.supporting_terms,
        units,
    )
    .await?;

    Ok(StageResult::executed(units))
}
