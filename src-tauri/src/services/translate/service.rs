use super::pipeline;
use super::types::{TranslatePipelineRequest, TranslatePipelineResponse};

pub async fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<TranslatePipelineResponse, String> {
    pipeline::run_translate_pipeline(request, |_| {}, |_, _| {}).await
}

pub async fn run_translate_summarize(
    request: &TranslatePipelineRequest,
) -> Result<(String, String), String> {
    pipeline::summarize_translate_style(request).await
}

pub async fn run_translate_with_style<G>(
    request: TranslatePipelineRequest,
    topic_summary: String,
    tone_strategy: String,
    on_batch_progress: &mut G,
) -> Result<TranslatePipelineResponse, String>
where
    G: FnMut(usize, usize),
{
    pipeline::run_translate_with_style(request, topic_summary, tone_strategy, on_batch_progress).await
}
