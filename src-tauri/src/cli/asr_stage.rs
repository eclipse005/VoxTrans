//! Stage 1 (ASR + forced alignment) and stage 2 (sentence segmentation +
//! beautify), plus their verbs.
//!
//! Both stages mirror what the desktop app does in
//! `commands::workspace::pipeline_steps::recognition` /
//! `execution_flow::execute_single_task_inner`, minus anything that exists only
//! to drive the UI (stage events, previews, review gates, hard-sub burn-in).

use std::path::{Path, PathBuf};

use clap::Args;
use serde_json::json;

use super::CliResult;
use super::artifacts::{
    SOURCE_JSON, SourceArtifact, SourceStats, WORDS_JSON, WordsArtifact, WordsStats,
    write_source_srt,
};
use super::context::{CliContext, CommonArgs, ContextRequest};
use super::report::{RunReport, StageTimer};

#[derive(Debug, Args)]
pub struct TranscribeArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// Media file to transcribe (audio or video).
    pub media: PathBuf,
}

#[derive(Debug, Args)]
pub struct SegmentArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// `words.json` produced by `transcribe`.
    pub words: PathBuf,
}

pub async fn run_transcribe(args: TranscribeArgs) -> CliResult<()> {
    let ctx = CliContext::new(ContextRequest {
        verb: "transcribe",
        common: &args.common,
        input_path: Some(&args.media),
        // ASR needs no LLM credentials.
        require_llm: false,
    })
    .await?;
    let mut report = RunReport::new(&ctx);

    let timer = StageTimer::start("transcribe");
    let outcome = match transcribe_stage(&ctx, &args.media).await {
        Ok(words) => {
            // Every unit came back from the resume tables: the stage ran but no
            // inference happened. Saying `cache` here is what lets an A/B loop
            // tell "the model saw my change" from "the previous run answered".
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
            Ok(())
        }
        Err(err) => Err(err),
    };
    report.finish(&ctx, outcome)
}

pub async fn run_segment(args: SegmentArgs) -> CliResult<()> {
    let ctx = CliContext::new(ContextRequest {
        verb: "segment",
        common: &args.common,
        input_path: Some(&args.words),
        require_llm: false,
    })
    .await?;
    let mut report = RunReport::new(&ctx);

    let timer = StageTimer::start("segment");
    let outcome = match async {
        let words = super::artifacts::load_words(&args.words)?;
        let source = segment_stage(&ctx, &words).await?;
        Ok::<SourceArtifact, String>(source)
    }
    .await
    {
        Ok(source) => {
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
            report.artifact(ctx.out_path(super::artifacts::SRC_SRT));
            Ok(())
        }
        Err(err) => Err(err),
    };
    report.finish(&ctx, outcome)
}

// ── stages ──────────────────────────────────────────────────────────────────

/// Events a blocking engine callback sends to the async side.
///
/// `CliContext` is only borrowed for the duration of a stage, so it cannot be
/// captured by a `spawn_blocking` closure or an `Arc<dyn Fn + 'static>`
/// callback. The callbacks emit these events instead, and the awaiting task
/// replays them through `ctx.progress` / `ctx.warn` (streaming, not batched)
/// while the engine thread runs.
enum StageEvent {
    Progress {
        phase: &'static str,
        done: usize,
        total: usize,
    },
    Warning(String),
}

/// Run a blocking stage call on the blocking pool and replay its progress on
/// the async side through [`CliContext::progress`] / [`CliContext::warn`].
///
/// The future completes only after the blocking thread returned and every queued
/// event was replayed; an engine panic is surfaced as an error, never lost.
async fn run_blocking_reported<T, F>(ctx: &CliContext, work: F) -> CliResult<T>
where
    T: Send + 'static,
    F: FnOnce(tokio::sync::mpsc::UnboundedSender<StageEvent>) -> Result<T, String>
        + Send
        + 'static,
{
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let join = tokio::task::spawn_blocking(move || work(sender));
    let replay = async {
        while let Some(event) = receiver.recv().await {
            match event {
                StageEvent::Progress { phase, done, total } => {
                    ctx.progress(phase, done, total, "")
                }
                StageEvent::Warning(message) => ctx.warn(&message),
            }
        }
    };
    let (joined, ()) = tokio::join!(join, replay);
    joined.map_err(|err| format!("blocking stage task failed: {err}"))?
}

/// Resume rows in exactly the shape `TranscribeRequest::precomputed_*` expects:
/// `(segment_index, text)` for ASR and `(segment_index, spans)` for alignment.
type PrecomputedAsrSegments = Vec<(usize, String)>;
type PrecomputedAlignmentSegments = Vec<(usize, Vec<crate::domain::AlignedSpan>)>;

/// Speech recognition + forced alignment.
///
/// Contract (mirrors `Step1AsrPipelineStep::run`):
///
/// 1. **Vocal separation first** when `ctx.runtime.enable_vocal_separation` is
///    on: `services::demucs::separate_vocals_blocking` (via `spawn_blocking`,
///    it shells out to the bundled `demucs` binary) and use the returned
///    `vocals_path` as the ASR input. Report progress as
///    `ctx.progress("separate", percent, 100, "")`. A demucs failure is an
///    error, not a silent fallback to the unseparated audio.
/// 2. **Resume state** from `ctx.unit_store()` (when present):
///    `load_asr_transcripts()` → `(segment_index, text)` and
///    `load_alignment_results(align_model)` → `(segment_index, spans)`, both fed
///    into `TranscribeRequest::precomputed_*`.
/// 3. **Run** `services::transcribe::transcribe_blocking` inside
///    `spawn_blocking` (it is a blocking engine call; the desktop app does the
///    same). Its progress callback reports `ctx.progress("asr", …)` /
///    `ctx.progress("align", …)` and — this is the resume invariant from
///    `domain::pipeline`'s module doc — persists each freshly computed unit
///    **inside the callback**, via
///    `UnitStore::save_asr_transcript` / `save_alignment_result` (use
///    `tokio::runtime::Handle::current().block_on(...)`; a save failure is a
///    reported warning, never a stage failure, because the next run simply
///    recomputes that unit).
/// 4. **Write** `<out>/words.json` (returned artifact; the caller records it).
///
/// The artifact records how many units came from the resume tables
/// (`cachedAsrSegments` / `cachedAlignSegments`), because a fully cached run
/// still says `source: "computed"` — it did run — and an A/B loop must be able
/// to tell whether the model actually saw the new code or answered from the
/// previous run's rows.
pub(crate) async fn transcribe_stage(ctx: &CliContext, media: &Path) -> CliResult<WordsArtifact> {
    // 1. Vocal separation first (mirrors `Step1AsrPipelineStep::run` +
    //    `execution_flow::separate_vocals_for_task`): the separated vocals are
    //    the ASR input. `separate_vocals_blocking` failure aborts the stage —
    //    it must never silently fall back to the unseparated audio.
    let asr_input = if ctx.runtime.enable_vocal_separation {
        ctx.progress("separate", 0, 100, "");
        let request = crate::services::demucs::SeparateVocalsRequest {
            task_id: ctx.task_id.clone(),
            audio_path: media.to_string_lossy().to_string(),
            model: ctx.runtime.demucs_model.clone(),
        };
        let response = run_blocking_reported(ctx, move |events| {
            crate::services::demucs::separate_vocals_blocking(request, |percent| {
                let _ = events.send(StageEvent::Progress {
                    phase: "separate",
                    done: percent as usize,
                    total: 100,
                });
            })
        })
        .await
        .map_err(|err| format!("vocal separation failed: {err}"))?;
        response.vocals_path
    } else {
        media.to_string_lossy().to_string()
    };

    // 2. Resume state: precomputed units from the task store (`None` for
    //    isolated / `--no-cache` runs). A load failure aborts, exactly like the
    //    GUI step — a broken cache must not be silently ignored.
    let (precomputed_asr, precomputed_alignment): (
        PrecomputedAsrSegments,
        PrecomputedAlignmentSegments,
    ) = match ctx.unit_store() {
        Some(unit_store) => {
            let asr = unit_store.load_asr_transcripts().await?;
            let alignment = unit_store
                .load_alignment_results(&ctx.runtime.align_model)
                .await?;
            (
                asr.into_iter()
                    .map(|row| (row.segment_index, row.text))
                    .collect(),
                alignment
                    .into_iter()
                    .map(|row| (row.segment_index, row.items))
                    .collect(),
            )
        }
        None => (Vec::new(), Vec::new()),
    };
    // Counted before the vectors move into the request; surfaced in the
    // artifact so a resume run is distinguishable from a fresh one.
    let cached_asr_segments = precomputed_asr.len();
    let cached_align_segments = precomputed_alignment.len();

    // 3. Run the blocking ASR/alignment engine. Progress is replayed through
    //    `ctx.progress`; each freshly computed unit is persisted inside the
    //    callback, the moment it exists (the resume invariant from
    //    `domain::pipeline`'s module doc). A failed save is a warning, never a
    //    stage failure: the next run simply recomputes that unit.
    let unit_store = ctx.unit_store();
    let align_model = ctx.runtime.align_model.clone();
    let request = crate::services::transcribe::TranscribeRequest {
        task_id: ctx.task_id.clone(),
        audio_path: asr_input,
        source_lang: ctx.source_lang.clone(),
        asr_model: ctx.runtime.asr_model.clone(),
        align_model: align_model.clone(),
        provider: ctx.runtime.provider.as_str().to_string(),
        chunk_target_seconds: ctx.runtime.chunk_target_seconds,
        model_dir: None,
        precomputed_asr_segments: precomputed_asr,
        precomputed_alignment,
    };

    let response = run_blocking_reported(ctx, move |events| {
        crate::services::transcribe::transcribe_blocking(
            request,
            move |stage, current, total, fresh_result| {
                use crate::services::transcribe::{FreshSegmentResult, TranscribeProgressStage};

                let phase = match stage {
                    TranscribeProgressStage::Asr => "asr",
                    TranscribeProgressStage::Align => "align",
                };
                let _ = events.send(StageEvent::Progress {
                    phase,
                    done: current,
                    total,
                });

                if let Some(fresh) = fresh_result {
                    match fresh {
                        FreshSegmentResult::Asr { segment_index, text } => {
                            if let Some(store) = unit_store.clone() {
                                let row = crate::domain::pipeline::AsrTranscriptRow {
                                    segment_index,
                                    text,
                                };
                                // `block_on` on the blocking-pool thread drives
                                // the save to completion before returning.
                                if let Err(err) = tokio::runtime::Handle::current()
                                    .block_on(store.save_asr_transcript(&row))
                                {
                                    let _ = events.send(StageEvent::Warning(format!(
                                        "failed to persist ASR unit {segment_index} (resume will recompute it): {err}"
                                    )));
                                }
                            }
                        }
                        FreshSegmentResult::Align { segment_index, items } => {
                            if let Some(store) = unit_store.clone() {
                                let row = crate::domain::pipeline::AlignmentResultRow {
                                    segment_index,
                                    items,
                                };
                                if let Err(err) = tokio::runtime::Handle::current()
                                    .block_on(store.save_alignment_result(&align_model, &row))
                                {
                                    let _ = events.send(StageEvent::Warning(format!(
                                        "failed to persist alignment unit {segment_index} (resume will recompute it): {err}"
                                    )));
                                }
                            }
                        }
                    }
                }
            },
        )
    })
    .await?;

    // 4. Write the stage artifact. `media_path` identifies the task input (the
    //    separated vocals stay an internal intermediate), matching the other
    //    CLI artifacts and `run.json`.
    let stats = words_stats_from_response(&response, cached_asr_segments, cached_align_segments);
    let words = WordsArtifact {
        task_id: ctx.task_id.clone(),
        media_path: ctx.input_path_str.clone(),
        source_lang: ctx.source_lang.clone(),
        asr_model: ctx.runtime.asr_model.clone(),
        align_model: ctx.runtime.align_model.clone(),
        text: response.text,
        aligned_text: response.aligned_text,
        words: response
            .words
            .into_iter()
            .map(|word| crate::commands::transcription::WordTokenCommandDto {
                start: word.start,
                end: word.end,
                word: word.word,
            })
            .collect(),
        vad_speech_segments: response.vad_speech_segments,
        stats,
    };
    ctx.write_json(WORDS_JSON, &words)?;
    Ok(words)
}

/// Sentence segmentation (step 2) + subtitle beautify.
///
/// Contract (mirrors `Step2SegmentsPipelineStep::run` +
/// `review_flow::materialize_source_sot`):
///
/// 1. `commands::transcription::build_source_sentences_with_progress` with the
///    artifact's `words` + `vad_speech_segments`, `ctx.source_lang` and
///    `ctx.frozen.subtitle_length_preset.as_str()`; progress →
///    `ctx.progress("segment", done, total, "")`.
/// 2. `SourceArtifact::build(ctx, &response.segments, stats)` — that helper
///    applies beautify (when `ctx.frozen.enable_subtitle_beautify`) exactly like
///    the app, and returns the SoT the translator must read. Pass
///    `micro_chunk_total` / `boundary_total` / `sentence_total` from the
///    response through `SourceStats`.
/// 3. Write `<out>/source.json` and the deliverable `<out>/src.srt` (via
///    [`write_source_srt`], which writes the *beautified* cues — the same rows
///    the app delivers), and return the artifact.
pub(crate) async fn segment_stage(ctx: &CliContext, words: &WordsArtifact) -> CliResult<SourceArtifact> {
    // 1. Step-2 sentence segmentation. The service's `on_progress` parameter is
    //    currently unused (the GUI passes `None` too) and its `Arc<dyn Fn +
    //    'static>` shape cannot borrow `ctx`, so progress is reported at the
    //    stage boundaries through `ctx.progress` instead.
    ctx.progress("segment", 0, 0, "");
    let request = crate::commands::transcription::BuildSourceSentencesCommandRequest {
        task_id: ctx.task_id.clone(),
        audio_path: words.media_path.clone(),
        source_lang: ctx.source_lang.clone(),
        subtitle_length_preset: ctx.frozen.subtitle_length_preset.as_str().to_string(),
        words: words.words.clone(),
        vad_speech_segments: words.vad_speech_segments.clone(),
    };
    let response =
        crate::commands::transcription::build_source_sentences_with_progress(request, None)
            .await?;
    ctx.progress("segment", response.sentence_total, response.sentence_total, "");

    // 2. Beautify + SoT shaping, exactly like `materialize_source_sot` does for
    //    the app (`SourceArtifact::build` owns the beautify pass).
    let stats = SourceStats {
        micro_chunk_total: response.micro_chunk_total,
        boundary_total: response.boundary_total,
        sentence_total: response.sentence_total,
        ..SourceStats::default()
    };
    let source = SourceArtifact::build(ctx, &response.segments, stats);

    // 3. Deliverables: the source SoT the translator reads, plus the beautified
    //    source SRT the app delivers.
    ctx.write_json(SOURCE_JSON, &source)?;
    write_source_srt(ctx, &source)?;
    Ok(source)
}

/// Stats helper kept here (not in `artifacts`) because only the ASR stage fills
/// it, and the field list tracks `TranscribeResponse` one-for-one. The two
/// cache counts are the exception: the response has no idea it was answering
/// from persisted rows, so the stage passes them in.
pub(crate) fn words_stats_from_response(
    response: &crate::services::transcribe::TranscribeResponse,
    cached_asr_segments: usize,
    cached_align_segments: usize,
) -> WordsStats {
    WordsStats {
        audio_duration_sec: response.audio_duration_sec,
        vad_elapsed_sec: response.vad_elapsed_sec,
        transcribe_elapsed_sec: response.transcribe_elapsed_sec,
        rtf_x: response.rtf_x,
        execution_provider: response.execution_provider.clone(),
        segment_total: response.segment_total,
        cached_asr_segments,
        cached_align_segments,
    }
}
