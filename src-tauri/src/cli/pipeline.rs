//! The end-to-end verb: media in, subtitles out, one command.
//!
//! It is a *composition* of the four stages — no pipeline logic of its own —
//! so `pipeline` cannot drift from running the verbs one after another. Each
//! stage writes its own artifacts; this module only sequences them and records
//! the timings in one report.

use std::path::PathBuf;

use clap::Args;
use serde_json::json;

use super::CliResult;
use super::artifacts::{BRIEFING_JSON, SOURCE_JSON, SRC_SRT, TRANSLATED_JSON, WORDS_JSON};
use super::context::{CliContext, CommonArgs, ContextRequest};
use super::report::{RunReport, StageTimer};
use crate::services::subtitle_srt::ExportSrtItem;

#[derive(Debug, Args)]
pub struct PipelineArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Media file to transcribe and translate.
    pub media: PathBuf,
    /// Reuse a saved briefing instead of running the terminology agent
    /// (`--no-terminology` skips it entirely).
    #[arg(long, value_name = "FILE", conflicts_with = "no_terminology")]
    pub briefing: Option<PathBuf>,
    /// Translate with the saved/user glossary only.
    #[arg(long)]
    pub no_terminology: bool,
}

/// Full run. Contract:
///
/// 1. Context with `verb: "pipeline"`, `input_path: media`, `require_llm: true`.
/// 2. `asr_stage::transcribe_stage` → `asr_stage::segment_stage` →
///    (terminology unless injected/skipped) `llm_stage::terminology_stage` →
///    `llm_stage::translate_stage`, each wrapped in a
///    [`super::report::StageTimer`] and recorded with the same `detail` keys the
///    individual verbs use, so a pipeline report can be compared against a
///    `translate` report.
/// 3. Artifact paths are recorded on the report; every artifact was written by
///    the stage that produced it (`words.json`, `source.json`, `src.srt`,
///    `briefing.json`, `trans.json` + the SRT variants).
/// 4. Fail fast: any stage error stops the run and is still reported (the report
///    is written, with the failing stage absent and `error` set).
pub async fn run_pipeline(args: PipelineArgs) -> CliResult<()> {
    let ctx = CliContext::new(ContextRequest {
        verb: "pipeline",
        common: &args.common,
        input_path: Some(&args.media),
        require_llm: true,
    })
    .await?;
    let mut report = RunReport::new(&ctx);

    // The stages run in sequence inside one `async` block: `?` stops at the
    // first failure while every timer already recorded stays in `report`, and
    // `finish` below writes that partial report with `error` set.
    let outcome = async {
        // ── Stage 1: ASR + forced alignment ─────────────────────────────
        let timer = StageTimer::start("transcribe");
        let words = super::asr_stage::transcribe_stage(&ctx, &args.media).await?;
        let fully_cached = words.stats.cached_asr_segments > 0
            && words.stats.cached_asr_segments >= words.stats.segment_total;
        timer.done_with_source(
            &mut report,
            if fully_cached { "cache" } else { "computed" },
            json!({
                "words": words.words.len(),
                "asrModel": words.asr_model,
                "alignModel": words.align_model,
                "audioDurationSec": words.stats.audio_duration_sec,
                "rtfX": words.stats.rtf_x,
                "provider": words.stats.execution_provider,
                "cachedAsrSegments": words.stats.cached_asr_segments,
                "cachedAlignSegments": words.stats.cached_align_segments,
            }),
        );
        report.artifact(ctx.out_path(WORDS_JSON));

        // ── Stage 2: segmentation + beautify ────────────────────────────
        let timer = StageTimer::start("segment");
        let source = super::asr_stage::segment_stage(&ctx, &words).await?;
        timer.done(
            &mut report,
            json!({
                "cues": source.stats.cue_total,
                "sentences": source.stats.sentence_total,
                "microChunks": source.stats.micro_chunk_total,
                "boundaries": source.stats.boundary_total,
                "beautified": source.stats.beautified,
                "preset": source.stats.subtitle_length_preset,
            }),
        );
        report.artifact(ctx.out_path(SOURCE_JSON));
        report.artifact(ctx.out_path(SRC_SRT));

        // ── Stage 3: terminology briefing ───────────────────────────────
        // Same precedence as `translate`: injected briefing > agent > none.
        // Only the agent path is a timed, recorded stage (and writes
        // briefing.json); the other two mirror `run_translate` exactly.
        let briefing = if let Some(path) = &args.briefing {
            Some(super::llm_stage::load_briefing(path)?)
        } else if args.no_terminology {
            None
        } else {
            let timer = StageTimer::start("terminology");
            let briefing = super::llm_stage::terminology_stage(&ctx, &source, None).await?;
            timer.done(
                &mut report,
                json!({
                    "windows": briefing.windows,
                    "terms": briefing.glossary.len(),
                    "styleGuideChars": briefing.style_guide.chars().count(),
                    "endReason": briefing.end_reason.as_str(),
                    "skipped": briefing.skipped,
                }),
            );
            report.artifact(ctx.out_path(BRIEFING_JSON));
            Some(briefing)
        };

        // ── Stage 4: translation + deliverables ─────────────────────────
        let timer = StageTimer::start("translate");
        let translated =
            super::llm_stage::translate_stage(&ctx, &source, briefing.as_ref()).await?;
        timer.done(
            &mut report,
            json!({
                "segments": translated.segments.len(),
                "batches": translated.stats.batch_total,
                "batchSize": translated.stats.batch_size,
                "glossaryTerms": translated.stats.glossary_terms,
                "userTerms": translated.stats.user_terms,
                "agentTerms": translated.stats.agent_glossary_terms,
                "elapsedSec": translated.stats.elapsed_sec,
            }),
        );
        report.artifact(ctx.out_path(TRANSLATED_JSON));
        for item in [
            ExportSrtItem::Source,
            ExportSrtItem::Target,
            ExportSrtItem::BilingualSourceFirst,
            ExportSrtItem::BilingualTargetFirst,
        ] {
            report.artifact(ctx.out_path(item.output_file_name()));
        }
        Ok(())
    }
    .await;

    report.finish(&ctx, outcome)
}
