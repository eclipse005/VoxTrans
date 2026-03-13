use crate::services::translation::domain::{SourceCue, StageResult, TranslationProfile, TranslationTerm};
use crate::services::translation::llm::TranslationLlmClient;

pub async fn run_stage(
    llm: &TranslationLlmClient,
    cues: &[SourceCue],
    source_language: &str,
    target_language: &str,
    preferred_translation_style: Option<&str>,
    terms: &[TranslationTerm],
) -> Result<StageResult<TranslationProfile>, String> {
    let profile = llm.summary_task(
        cues,
        source_language,
        target_language,
        preferred_translation_style,
        terms,
    )
    .await?;

    Ok(StageResult::executed(profile))
}
