use crate::services::translation::domain::{
    HotwordHint, SentenceUnit, StageResult, TranslatedUnit, TranslationProfile,
};
use crate::services::translation::llm::TranslationLlmClient;

pub async fn run_stage(
    llm: &TranslationLlmClient,
    source_language: &str,
    target_language: &str,
    summary: &TranslationProfile,
    hotword_hint: Option<&HotwordHint>,
    units: &[SentenceUnit],
) -> Result<StageResult<Vec<TranslatedUnit>>, String> {
    let units = llm
        .translate_sentences(
            source_language,
            target_language,
            Some(summary.translation_style.as_str()),
            Some(summary.topic_summary.as_str()),
            &summary.primary_terms,
            &summary.supporting_terms,
            hotword_hint,
            units,
        )
        .await?;

    Ok(StageResult::executed(units))
}
