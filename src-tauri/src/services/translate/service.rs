use super::pipeline;
use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub async fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    pipeline::run_translate_pipeline(request, |_| {}, |_, _| {}).await
}

pub async fn run_translate_summarize(
    request: &TranslatePipelineRequest,
) -> Result<(String, Vec<super::types::TranslateTerminologyEntry>, usize, usize), String> {
    pipeline::summarize_translate_theme(request).await
}

pub async fn run_translate_with_theme<G>(
    request: TranslatePipelineRequest,
    theme: String,
    terminology_entries: Vec<super::types::TranslateTerminologyEntry>,
    on_batch_progress: &mut G,
) -> Result<TranslatePipelineResponse, String>
where
    G: FnMut(usize, usize),
{
    pipeline::run_translate_with_theme(request, theme, terminology_entries, on_batch_progress).await
}
