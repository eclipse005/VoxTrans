use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TranslationToken {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentInput {
    pub segment: String,
    pub start: f64,
    pub end: f64,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct TranslationTerminologyEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub segments: Vec<TranslationSegmentInput>,
    pub theme_summary: String,
    pub terminology_entries: Vec<TranslationTerminologyEntry>,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    pub batch_size: usize,
    pub unit_store: Option<crate::services::pipeline::UnitStore>,
}

#[derive(Debug, Clone)]
pub struct TranslationSegmentOutput {
    pub segment_id: usize,
    pub start: f64,
    pub end: f64,
    pub source: String,
    pub translation: String,
    pub tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub struct BuildTranslationLayerResponse {
    pub batch_size: usize,
    pub batch_total: usize,
    pub segment_total: usize,
    pub segments: Vec<TranslationSegmentOutput>,
}

/// Structured progress for translation.
///
/// Emitted mid-batch (token stream, throttled) and when a batch completes.
/// `partial_outputs` is a full snapshot of all segments rebuilt from the
/// cumulative translations so far: finished lines are complete; the line
/// currently streaming may grow character-by-character; the rest keep an
/// empty translation until their batch runs.
#[derive(Debug, Clone)]
pub struct TranslationProgress {
    pub done: usize,
    pub total: usize,
    pub partial_outputs: Vec<Arc<TranslationSegmentOutput>>,
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedSegment {
    pub(super) segment_id: usize,
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) source: String,
    pub(super) tokens: Vec<TranslationToken>,
}

#[derive(Debug, Clone)]
pub(super) struct BatchWindow {
    pub(super) batch_id: usize,
    pub(super) local_ids: Vec<usize>,
    pub(super) local_to_global: Vec<usize>,
    /// Current-batch lines as the LLM sees them (1-based local ids).
    pub(super) current_lines: Arc<[crate::services::prompts::translation::TranslationPromptLine]>,
    /// (segment_id, source) for up to PREV_CONTEXT_LINES before the batch.
    pub(super) prev_lines: Arc<[(usize, String)]>,
    /// (segment_id, source) for up to NEXT_CONTEXT_LINES after the batch.
    pub(super) next_lines: Arc<[(usize, String)]>,
    /// Terminology entries selected for this batch.
    pub(super) terms: Arc<[crate::services::prompts::translation::TranslationPromptTerm]>,
    pub(super) theme_summary: String,
    pub(super) source_lang: String,
    pub(super) target_lang: String,
}

impl BatchWindow {
    /// Build this batch's translation prompt.
    ///
    /// previousLines become `"source → translation"` once the predecessor
    /// batch has committed. nextLines stay source-only even on resume — the
    /// future batch is context, not an answer key.
    pub(super) fn build_prompt(
        &self,
        known_translations: &std::collections::HashMap<usize, String>,
        extra_terms: &[crate::services::prompts::translation::TranslationPromptTerm],
        established_names: &[crate::services::prompts::translation::TranslationNameExample],
    ) -> String {
        let prev_lines = self
            .prev_lines
            .iter()
            .map(|(id, source)| match known_translations.get(id) {
                Some(translation) => format!("{} → {}", source, translation),
                None => source.clone(),
            })
            .collect::<Vec<_>>();
        let next_lines = self
            .next_lines
            .iter()
            .map(|(_, source)| source.clone())
            .collect::<Vec<_>>();
        let terms = merge_terms(&self.terms, extra_terms);
        crate::services::prompts::translation::build_batch_translate_prompt(
            &self.source_lang,
            &self.target_lang,
            &self.theme_summary,
            &prev_lines,
            &self.current_lines,
            &next_lines,
            &terms,
            established_names,
        )
    }
}

fn merge_terms(
    primary: &[crate::services::prompts::translation::TranslationPromptTerm],
    extra: &[crate::services::prompts::translation::TranslationPromptTerm],
) -> Vec<crate::services::prompts::translation::TranslationPromptTerm> {
    let mut out = primary.to_vec();
    let mut seen: std::collections::HashSet<String> = primary
        .iter()
        .map(|t| {
            t.source
                .to_lowercase()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect()
        })
        .collect();
    for term in extra {
        let key: String = term
            .source
            .to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(term.clone());
    }
    out
}

