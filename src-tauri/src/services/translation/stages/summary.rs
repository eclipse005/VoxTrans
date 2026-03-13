use crate::services::translation::domain::{SourceCue, TranslationProfile, TranslationTerm};
use crate::services::translation::llm::TranslationLlmClient;

pub async fn run(
    llm: &TranslationLlmClient,
    cues: &[SourceCue],
    source_language: &str,
    target_language: &str,
    preferred_translation_style: Option<&str>,
    terms: &[TranslationTerm],
) -> Result<TranslationProfile, String> {
    llm.summary_task(
        cues,
        source_language,
        target_language,
        preferred_translation_style,
        terms,
    )
    .await
}
