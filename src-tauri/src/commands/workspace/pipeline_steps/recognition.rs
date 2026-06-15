use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::runtime::Handle;

use crate::services::pipeline::{CheckpointPolicy, PipelineStep, StepContext};

use super::super::progress::report_task_stage;
use super::super::TaskStage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::commands::workspace) struct Step1AsrArtifact {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    #[serde(default)]
    pub(in crate::commands::workspace) text: String,
    #[serde(default)]
    pub(in crate::commands::workspace) aligned_text: String,
    pub(in crate::commands::workspace) words:
        Vec<crate::commands::transcription::WordTokenCommandDto>,
    #[serde(default)]
    pub(in crate::commands::workspace) vad_speech_segments: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step1AsrPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) asr_model: String,
    pub(in crate::commands::workspace) align_model: String,
    pub(in crate::commands::workspace) provider: String,
    pub(in crate::commands::workspace) chunk_target_seconds: u32,
    pub(in crate::commands::workspace) app: AppHandle,
}

#[async_trait]
impl PipelineStep for Step1AsrPipelineStep {
    type Output = Step1AsrArtifact;

    fn name(&self) -> &'static str {
        "step_01_asr"
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.task_id.trim().is_empty()
            || output.media_path.trim().is_empty()
            || output.words.is_empty()
        {
            return Err("invalid step1 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let unit_store = ctx.unit_store();

        let precomputed_asr = unit_store.load_asr_transcripts().await?;
        let precomputed_asr_segments: Vec<(usize, String)> = precomputed_asr
            .into_iter()
            .map(|row| (row.segment_index, row.text))
            .collect();

        let precomputed_alignment = unit_store.load_alignment_results().await?;
        let precomputed_alignment_segments: Vec<(
            usize,
            qwen_forced_aligner_rs::ForcedAlignResult,
        )> = precomputed_alignment
            .into_iter()
            .map(|row| {
                (
                    row.segment_index,
                    qwen_forced_aligner_rs::ForcedAlignResult {
                        items: row.items,
                        output_ids: Vec::new(),
                        raw_timestamp_ms: Vec::new(),
                        fixed_timestamp_ms: Vec::new(),
                    },
                )
            })
            .collect();

        let unit_store_cb = unit_store.clone();

        let task_id_owned = self.task_id.clone();
        let app_for_progress = self.app.clone();
        let media_path = self.media_path.clone();
        let provider = self.provider.clone();
        let chunk_target_seconds = self.chunk_target_seconds;
        let transcribe_request = crate::services::transcribe::TranscribeRequest {
            task_id: task_id_owned.clone(),
            audio_path: media_path.clone(),
            source_lang: self.source_lang.clone(),
            asr_model: self.asr_model.clone(),
            align_model: self.align_model.clone(),
            provider,
            chunk_target_seconds,
            model_dir: None,
            precomputed_asr_segments,
            precomputed_alignment: precomputed_alignment_segments,
        };
        let transcribe_response = tauri::async_runtime::spawn_blocking(move || {
            let us = unit_store_cb;
            crate::services::transcribe::transcribe_blocking(
                transcribe_request,
                move |stage, current, total, fresh_result| {
                    let task_stage = match stage {
                        crate::services::transcribe::TranscribeProgressStage::Asr => {
                            TaskStage::Recognizing
                        }
                        crate::services::transcribe::TranscribeProgressStage::Align => {
                            TaskStage::Aligning
                        }
                    };
                    let report = report_task_stage(
                        &app_for_progress,
                        &task_id_owned,
                        task_stage,
                        format!("{current}/{total}"),
                        current as u32,
                        total as u32,
                    );
                    match Handle::try_current() {
                        Ok(handle)
                            if handle.runtime_flavor()
                                == tokio::runtime::RuntimeFlavor::MultiThread =>
                        {
                            let _ = tokio::task::block_in_place(|| {
                                handle.block_on(async {
                                    let _ = report.await;
                                    match fresh_result {
                                        Some(crate::services::transcribe::FreshSegmentResult::Asr {
                                            segment_index,
                                            text,
                                        }) => {
                                            let row =
                                                crate::services::pipeline::AsrTranscriptRow {
                                                    segment_index,
                                                    text,
                                                };
                                            let _ = us.save_asr_transcript(&row).await;
                                        }
                                        Some(crate::services::transcribe::FreshSegmentResult::Align {
                                            segment_index,
                                            items,
                                        }) => {
                                            let row =
                                                crate::services::pipeline::AlignmentResultRow {
                                                    segment_index,
                                                    items,
                                                };
                                            let _ = us.save_alignment_result(&row).await;
                                        }
                                        None => {}
                                    }
                                })
                            });
                        }
                        _ => {}
                    }
                },
            )
        })
        .await
        .map_err(|err| err.to_string())??;

        let words = transcribe_response
            .words
            .iter()
            .map(|word| crate::commands::transcription::WordTokenCommandDto {
                start: word.start,
                end: word.end,
                word: word.word.clone(),
            })
            .collect::<Vec<_>>();
        Ok(Step1AsrArtifact {
            task_id: self.task_id.clone(),
            media_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            text: transcribe_response.text,
            aligned_text: transcribe_response.aligned_text,
            words,
            vad_speech_segments: transcribe_response.vad_speech_segments,
        })
    }
}

#[derive(Debug, Clone)]
pub(in crate::commands::workspace) struct Step2SegmentsPipelineStep {
    pub(in crate::commands::workspace) task_id: String,
    pub(in crate::commands::workspace) media_path: String,
    pub(in crate::commands::workspace) source_lang: String,
    pub(in crate::commands::workspace) subtitle_length_preset: String,
    pub(in crate::commands::workspace) use_subtitle_layout_split: bool,
    pub(in crate::commands::workspace) words:
        Vec<crate::commands::transcription::WordTokenCommandDto>,
    pub(in crate::commands::workspace) vad_speech_segments: Vec<(f64, f64)>,
}

#[async_trait]
impl PipelineStep for Step2SegmentsPipelineStep {
    type Output = Vec<crate::commands::transcription::GroupedSentenceSegmentCommandDto>;

    fn name(&self) -> &'static str {
        "step_02_segments"
    }

    fn policy(&self) -> CheckpointPolicy {
        CheckpointPolicy::SkipIfExists
    }

    fn validate(&self, output: &Self::Output) -> Result<(), String> {
        if output.is_empty() {
            return Err("invalid step2 artifact".to_string());
        }
        Ok(())
    }

    async fn run(&self, _ctx: &StepContext<'_>) -> Result<Self::Output, String> {
        let step2_request = crate::commands::transcription::BuildSourceSentencesCommandRequest {
            task_id: self.task_id.clone(),
            audio_path: self.media_path.clone(),
            source_lang: self.source_lang.clone(),
            subtitle_length_preset: self.subtitle_length_preset.clone(),
            use_subtitle_layout_split: self.use_subtitle_layout_split,
            words: self.words.clone(),
            vad_speech_segments: self.vad_speech_segments.clone(),
        };
        let step2_response = crate::commands::transcription::build_source_sentences_with_progress(
            step2_request,
            None,
        )
        .await?;
        Ok(step2_response.segments)
    }
}
