//! Stage 3 (terminology briefing) and stage 4 (translation), plus their verbs.
//!
//! These mirror `commands::workspace::translation_flow` — including its
//! fail-open terminology agent and its user-terms-win merge — so a CLI run
//! reproduces what the GUI would have produced for the same input.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::Args;
use serde_json::json;

use crate::commands::translate_terms::normalize_command_terminology_entries;
use crate::services::subtitle_srt::{ExportSrtItem, write_variants_to_directory};
use crate::services::terminology_agent::{
    EndReason, GlossaryEntry, TerminologyAgentInput, TerminologyBriefing,
    merge_glossary_user_priority, run_terminology_briefing,
};
use crate::services::translation::{
    BuildTranslationLayerRequest, TranslationPlan, TranslationProgress, TranslationSegmentInput,
    TranslationTerminologyEntry,
};

use super::CliResult;
use super::artifacts::{
    BRIEFING_JSON, SourceArtifact, TRANSLATED_JSON, TranslateStats, TranslatedArtifact,
    TranslatedSegment,
};
use super::context::{CliContext, CommonArgs, ContextRequest};
use super::report::{RunReport, StageTimer};

#[derive(Debug, Args)]
pub struct TerminologyArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// `.srt`, `source.json` or `words.json` to extract terminology from.
    pub input: PathBuf,
    /// Reuse a saved briefing instead of calling the model (accepts a
    /// `briefing.json` from an earlier run, or a raw `TerminologyBriefing`).
    #[arg(long, value_name = "FILE")]
    pub briefing: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct TranslateArgs {
    #[command(flatten)]
    pub common: CommonArgs,
    /// `.srt`, `source.json` or `words.json` to translate.
    pub input: PathBuf,
    /// Apply this briefing (from `terminology`/`briefing.json`) instead of
    /// running the terminology agent. Mutually exclusive with
    /// `--no-terminology`.
    #[arg(long, value_name = "FILE", conflicts_with = "no_terminology")]
    pub briefing: Option<PathBuf>,
    /// Skip the terminology agent: translate with the saved/user glossary only.
    #[arg(long)]
    pub no_terminology: bool,
    /// Plan the prompts and stop: nothing is sent, nothing is billed. The
    /// terminology agent is skipped unless `--briefing` supplies one — an
    /// agent run spends real tokens on its windows, which a dry run must not
    /// do. Prompts land in `<out>/prompts.json`.
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run_terminology(args: TerminologyArgs) -> CliResult<()> {
    let ctx = CliContext::new(ContextRequest {
        verb: "terminology",
        common: &args.common,
        input_path: Some(&args.input),
        // The agent needs the LLM; it fails open (skipped) but an unconfigured
        // endpoint is a usage error worth catching up front.
        require_llm: args.briefing.is_none(),
    })
    .await?;
    let mut report = RunReport::new(&ctx);

    let timer = StageTimer::start("terminology");
    let outcome = match async {
        let source = super::artifacts::source_from_input(&ctx, &args.input).await?;
        let briefing = terminology_stage(&ctx, &source, args.briefing.as_deref()).await?;
        Ok::<(SourceArtifact, TerminologyBriefing), String>((source, briefing))
    }
    .await
    {
        Ok((source, briefing)) => {
            let detail = json!({
                "cues": source.segments.len(),
                "windows": briefing.windows,
                "terms": briefing.glossary.len(),
                "styleGuideChars": briefing.style_guide.chars().count(),
                "endReason": briefing.end_reason.as_str(),
                "skipped": briefing.skipped,
            });
            if args.briefing.is_some() {
                timer.done_with_source(&mut report, "cache", detail);
            } else {
                timer.done(&mut report, detail);
            }
            report.artifact(ctx.out_path(BRIEFING_JSON));
            Ok(())
        }
        Err(err) => Err(err),
    };
    report.finish(&ctx, outcome)
}

pub async fn run_translate(args: TranslateArgs) -> CliResult<()> {
    let ctx = CliContext::new(ContextRequest {
        verb: "translate",
        common: &args.common,
        input_path: Some(&args.input),
        // A dry run still needs the endpoint configured: the point is to check
        // the plan a real run would send.
        require_llm: true,
    })
    .await?;
    let mut report = RunReport::new(&ctx);

    let outcome = async {
        let source = super::artifacts::source_from_input(&ctx, &args.input).await?;

        // Terminology: injected briefing > agent > user terms only. This is the
        // same precedence the app has (frozen user glossary always wins), with
        // one CLI-only exception: a `--dry-run` bills nothing, so it skips the
        // agent unless the caller injects a briefing.
        let briefing = if let Some(path) = &args.briefing {
            Some(load_briefing(path)?)
        } else if args.no_terminology || args.dry_run {
            if args.dry_run {
                report.warn(
                    "--dry-run: terminology agent skipped, so the plan has no glossary; \
                     pass --briefing to plan with one",
                );
            }
            None
        } else {
            let timer = StageTimer::start("terminology");
            let briefing = terminology_stage(&ctx, &source, None).await?;
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

        if args.dry_run {
            let timer = StageTimer::start("plan");
            let (plan, stats) = plan_translate(&ctx, &source, briefing.as_ref())?;
            let prompts: Vec<_> = plan
                .windows
                .iter()
                .map(|window| {
                    json!({
                        "batchId": window.batch_id,
                        "unitIds": window.unit_ids,
                        "slotIds": window.slot_ids,
                        "prompt": window.prompt,
                    })
                })
                .collect();
            let path = ctx.write_json(
                "prompts.json",
                &json!({
                    "taskId": ctx.task_id,
                    "sourceLang": source.source_lang,
                    "targetLang": source.target_lang,
                    "unitTotal": plan.unit_total,
                    "windowTotal": plan.window_total,
                    "glossary": stats,
                    "windows": prompts,
                }),
            )?;
            timer.done(
                &mut report,
                json!({
                    "units": plan.unit_total,
                    "windows": plan.window_total,
                    "glossaryTerms": stats.merged_terms,
                }),
            );
            report.artifact(path);
            return Ok(());
        }

        let timer = StageTimer::start("translate");
        let translated = translate_stage(&ctx, &source, briefing.as_ref()).await?;
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

        // Deliverables (trans.json + the four SRT variants) are written by the
        // stage itself, so `pipeline` gets the same outputs for free.
        report.artifact(ctx.out_path(TRANSLATED_JSON));
        report.artifact(
            ctx.out_path(crate::services::subtitle_srt::ExportSrtItem::Source.output_file_name()),
        );
        report.artifact(
            ctx.out_path(crate::services::subtitle_srt::ExportSrtItem::Target.output_file_name()),
        );
        report.artifact(ctx.out_path(
            crate::services::subtitle_srt::ExportSrtItem::BilingualSourceFirst.output_file_name(),
        ));
        report.artifact(ctx.out_path(
            crate::services::subtitle_srt::ExportSrtItem::BilingualTargetFirst.output_file_name(),
        ));
        Ok(())
    }
    .await;

    report.finish(&ctx, outcome)
}

// ── stages ──────────────────────────────────────────────────────────────────

/// Glossary accounting shared by the real run and the dry run, so the report
/// says the same thing in both.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GlossaryStats {
    pub user_terms: usize,
    pub agent_terms: usize,
    pub merged_terms: usize,
    pub style_guide_chars: usize,
}

/// Terminology extraction for this source artifact.
///
/// Contract (mirrors `translation_flow::resolve_terminology_for_translation`):
///
/// 1. When `inject` is `Some(path)`: load that briefing ([`load_briefing`]) and
///    return it — the agent is not called (the caller records the stage as
///    `cache`). A briefing that is `skipped`/empty is still returned as-is.
/// 2. Otherwise call `services::terminology_agent::run_terminology_briefing`
///    with `cues: source.cues()`, `user_terms: ctx.user_terms` as
///    `GlossaryEntry`, `title` = input file stem,
///    `store: ctx.store.clone()` when `--task` was given AND caching is on —
///    the store carries both the token accounting and the merged-briefing
///    cache, and a `--no-cache` (A/B) run must not let a cached briefing
///    answer for the model (see `CliContext::unit_store`'s isolation rule).
///    Endpoint from `ctx.runtime.translate_*`. Progress →
///    `ctx.progress("terminology", done, estimated, "")`.
/// 3. The agent never returns `Err` — it fails open to `skipped`. Do not invent
///    an error path: a skipped briefing means translation continues with the
///    user's glossary only, exactly like the app.
/// 4. Write `<out>/briefing.json` and return it.
pub(crate) async fn terminology_stage(
    ctx: &CliContext,
    source: &SourceArtifact,
    inject: Option<&std::path::Path>,
) -> CliResult<TerminologyBriefing> {
    let briefing = if let Some(path) = inject {
        load_briefing(path)?
    } else {
        let title = ctx
            .input_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| ctx.task_id.clone());
        let user_terms = ctx
            .user_terms
            .iter()
            .map(|entry| GlossaryEntry::new(&entry.source, &entry.target, &entry.note))
            .collect::<Vec<_>>();

        // `run_terminology_briefing` never returns Err: a failed or panicking
        // extraction comes back as `skipped`, and translation continues with
        // the user glossary only. Do not add an error path here.
        run_terminology_briefing(
            TerminologyAgentInput {
                task_id: ctx.task_id.clone(),
                media_path: ctx.input_path_str.clone(),
                title,
                source_lang: ctx.source_lang.clone(),
                target_lang: ctx.target_lang.clone(),
                cues: source.cues(),
                user_terms,
                api_key: ctx.runtime.translate_api_key.clone(),
                base_url: ctx.runtime.translate_base_url.clone(),
                model: ctx.runtime.translate_model.clone(),
                // Token accounting and the merged-briefing cache both key off
                // a task row; an isolated or `--no-cache` run must read/write
                // neither, matching `CliContext::unit_store`'s isolation rule.
                store: if ctx.task_registered && !ctx.no_cache {
                    ctx.store.clone()
                } else {
                    None
                },
            },
            |done, estimated, _width| ctx.progress("terminology", done, estimated, ""),
        )
        .await
    };

    ctx.write_json(BRIEFING_JSON, &briefing)?;
    Ok(briefing)
}

/// Load a briefing from disk: either a `briefing.json` produced by this CLI
/// (`{"glossary": …, "styleGuide": …}`) or a raw `TerminologyBriefing`.
pub(crate) fn load_briefing(path: &std::path::Path) -> CliResult<TerminologyBriefing> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    // Canonical shape: what the agent and a `terminology` run persist
    // (snake_case, every bookkeeping field written).
    if let Ok(briefing) = serde_json::from_str::<TerminologyBriefing>(&raw) {
        return Ok(briefing);
    }

    // Documented `briefing.json` shape: camelCase with the bookkeeping fields
    // optional, so a hand-written or GUI-produced file loads too.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BriefingFile {
        #[serde(default)]
        glossary: Vec<GlossaryEntry>,
        #[serde(default, alias = "style_guide")]
        style_guide: String,
        #[serde(default)]
        windows: usize,
        #[serde(default, alias = "end_reason")]
        end_reason: Option<String>,
        #[serde(default)]
        skipped: Option<bool>,
        #[serde(default, alias = "skip_reason")]
        skip_reason: Option<String>,
    }

    let file: BriefingFile = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "failed to parse {} as a terminology briefing: {e}",
            path.display()
        )
    })?;

    let skipped = file
        .skipped
        .unwrap_or_else(|| file.glossary.is_empty() && file.style_guide.trim().is_empty());
    let end_reason = match file.end_reason.as_deref() {
        Some("completed") => EndReason::Completed,
        Some("early_exit") | Some("earlyExit") => EndReason::EarlyExit,
        Some("llm_error") | Some("llmError") => EndReason::LlmError,
        Some("skipped") => EndReason::Skipped,
        _ if skipped => EndReason::Skipped,
        _ => EndReason::Completed,
    };

    Ok(TerminologyBriefing {
        glossary: file.glossary,
        style_guide: file.style_guide,
        windows: file.windows,
        end_reason,
        skipped,
        skip_reason: file.skip_reason,
    })
}

/// Build the exact request a translation run (or a dry run) would send, plus
/// the glossary accounting for the report.
///
/// Contract (mirrors `translation_flow` + `translate_translation`):
///
/// - Segments: `source.step2()` → `TranslationSegmentInput` (keep
///   `semantic_sentence_id` and the word tokens; dropping either degrades
///   sentence-level translation to per-cue translation).
/// - Glossary: `ctx.user_terms` plus, when a briefing is present,
///   `merge_glossary_user_priority(&user_terms, &briefing.glossary,
///   &source.haystack())` — user terms win on the same normalized source,
///   exactly like the app.
/// - Style guide: `ctx.style_guide` when non-empty (the flag wins), else
///   `briefing.style_guide`.
/// - Endpoint/concurrency from `ctx.runtime`, `batch_size` from `ctx.batch_size`,
///   `unit_store: ctx.unit_store()` (resume + per-batch checkpointing; `None`
///   for isolated or `--no-cache` runs).
pub(crate) fn build_translate_request(
    ctx: &CliContext,
    source: &SourceArtifact,
    briefing: Option<&TerminologyBriefing>,
) -> CliResult<(BuildTranslationLayerRequest, GlossaryStats)> {
    // Same normalization the command layer applies before mapping DTOs into
    // the service request.
    let user_entries = normalize_command_terminology_entries(ctx.user_terms.clone());
    let user_terms = user_entries
        .iter()
        .map(|entry| GlossaryEntry::new(&entry.source, &entry.target, &entry.note))
        .collect::<Vec<_>>();

    // User glossary always wins on a shared source; agent rows are grounded
    // against the transcript (see `merge_glossary_user_priority`).
    let glossary = match briefing {
        Some(briefing) => {
            merge_glossary_user_priority(&user_terms, &briefing.glossary, &source.haystack())
        }
        None => user_terms.clone(),
    };

    // An explicit `--style-guide` file overrides the agent's style guide.
    let style_guide = if ctx.style_guide.trim().is_empty() {
        briefing.map(|b| b.style_guide.clone()).unwrap_or_default()
    } else {
        ctx.style_guide.clone()
    };

    let stats = GlossaryStats {
        user_terms: user_terms.len(),
        agent_terms: briefing.map(|b| b.glossary.len()).unwrap_or(0),
        merged_terms: glossary.len(),
        style_guide_chars: style_guide.chars().count(),
    };

    let request = BuildTranslationLayerRequest {
        task_id: ctx.task_id.clone(),
        media_path: ctx.input_path_str.clone(),
        source_lang: ctx.source_lang.clone(),
        target_lang: ctx.target_lang.clone(),
        // `to_translation_inputs` keeps `semantic_sentence_id` and the word
        // tokens, so sentence-level units survive the CLI path.
        segments: to_translation_inputs(source),
        style_guide,
        terminology_entries: to_translation_terms(&glossary),
        translate_api_key: ctx.runtime.translate_api_key.clone(),
        translate_base_url: ctx.runtime.translate_base_url.clone(),
        translate_model: ctx.runtime.translate_model.clone(),
        llm_concurrency: ctx.runtime.llm_concurrency.max(1),
        batch_size: ctx.batch_size,
        unit_store: ctx.unit_store(),
    };
    Ok((request, stats))
}

/// Run the translation layer, write its artifacts, and return them.
///
/// Writes: `<out>/trans.json` plus the four SRT variants the app delivers
/// (`src.srt`, `trans.srt`, `src_trans.srt`, `trans_src.srt`, from
/// `TranslatedArtifact::srt_segments(&source)` — which fails loudly rather than
/// emitting a blank line for a cue the layer never translated).
///
/// Contract: build the request with [`build_translate_request`], call
/// `services::translation::build_translation_layer_with_progress`, report
/// progress as `ctx.progress("translate", done, total, "")`, then map the
/// response into [`TranslatedArtifact`] (1-based `segment_id`s, `stats` filled
/// from `GlossaryStats` + elapsed time + `batch_total`/`batch_size`).
pub(crate) async fn translate_stage(
    ctx: &CliContext,
    source: &SourceArtifact,
    briefing: Option<&TerminologyBriefing>,
) -> CliResult<TranslatedArtifact> {
    let (request, glossary) = build_translate_request(ctx, source, briefing)?;

    // The progress callback must be `'static`, so it cannot borrow `ctx` to
    // call `ctx.progress` itself. Forward updates through a channel and render
    // them here, where the context is still in scope.
    let (progress_tx, mut progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<TranslationProgress>();
    let on_progress: Arc<dyn Fn(TranslationProgress) + Send + Sync> = Arc::new(move |update| {
        let _ = progress_tx.send(update);
    });

    let started = Instant::now();
    let call = crate::services::translation::build_translation_layer_with_progress(
        request,
        Some(on_progress),
    );
    tokio::pin!(call);
    let response = loop {
        tokio::select! {
            result = &mut call => break result?,
            Some(update) = progress_rx.recv() => {
                ctx.progress("translate", update.done, update.total, "");
            }
        }
    };
    // The channel can still hold updates when the call resolves (last batch,
    // or a final token-stream frame): drain them so the summary does not end on
    // a stale `n-1/n` line.
    while let Ok(update) = progress_rx.try_recv() {
        ctx.progress("translate", update.done, update.total, "");
    }
    let elapsed_sec = started.elapsed().as_secs_f64();

    let stats = TranslateStats {
        batch_total: response.batch_total,
        batch_size: response.batch_size,
        elapsed_sec,
        glossary_terms: glossary.merged_terms,
        user_terms: glossary.user_terms,
        agent_glossary_terms: glossary.agent_terms,
        agent_end_reason: briefing
            .map(|b| b.end_reason.as_str().to_string())
            .unwrap_or_default(),
        style_guide_chars: glossary.style_guide_chars,
    };
    let artifact = translated_artifact(ctx, response, stats);

    // trans.json first: the canonical artifact survives a later export failure.
    ctx.write_json(TRANSLATED_JSON, &artifact)?;

    // The four deliverables the app ships. `srt_segments` errors on a cue the
    // layer never translated instead of emitting a blank line.
    let deliverable = artifact.srt_segments(source)?;
    write_variants_to_directory(
        &ctx.out_dir,
        &deliverable,
        &[
            ExportSrtItem::Source,
            ExportSrtItem::Target,
            ExportSrtItem::BilingualSourceFirst,
            ExportSrtItem::BilingualTargetFirst,
        ],
    )?;
    Ok(artifact)
}

/// Dry run: the exact window/unit/prompt plan without touching the network.
///
/// Contract: build the request with [`build_translate_request`] and hand it to
/// `services::translation::plan_translation_windows` (a `pub(crate)` seam the
/// stage worker adds next to `build_translation_layer_with_progress`; it reuses
/// the same unit grouping and window builder, so the plan cannot drift from what
/// a real run sends).
pub(crate) fn plan_translate(
    ctx: &CliContext,
    source: &SourceArtifact,
    briefing: Option<&TerminologyBriefing>,
) -> CliResult<(TranslationPlan, GlossaryStats)> {
    let (request, stats) = build_translate_request(ctx, source, briefing)?;
    // Pure prompt planning: no LLM client is constructed, so `--dry-run`
    // never touches the network.
    let plan = crate::services::translation::plan_translation_windows(&request)?;
    Ok((plan, stats))
}

/// Helper the stage uses to convert glossary entries into request terms.
pub(crate) fn to_translation_terms(
    entries: &[crate::services::terminology_agent::GlossaryEntry],
) -> Vec<TranslationTerminologyEntry> {
    entries
        .iter()
        .map(|entry| TranslationTerminologyEntry {
            source: entry.source.clone(),
            target: entry.target.clone(),
            note: entry.note.clone(),
        })
        .collect()
}

/// Helper the stage uses to convert a source artifact into translation inputs.
pub(crate) fn to_translation_inputs(source: &SourceArtifact) -> Vec<TranslationSegmentInput> {
    source
        .segments
        .iter()
        .map(|segment| TranslationSegmentInput {
            segment: segment.segment.clone(),
            sentence_id: segment.semantic_sentence_id,
            start: segment.start,
            end: segment.end,
            tokens: segment
                .tokens
                .iter()
                .map(|token| crate::services::translation::TranslationToken {
                    text: token.text.clone(),
                    start: token.start,
                    end: token.end,
                })
                .collect(),
        })
        .collect()
}

/// Convert the layer's response into the artifact (1-based ids preserved).
pub(crate) fn translated_artifact(
    ctx: &CliContext,
    response: crate::services::translation::BuildTranslationLayerResponse,
    stats: TranslateStats,
) -> TranslatedArtifact {
    TranslatedArtifact {
        task_id: ctx.task_id.clone(),
        media_path: ctx.input_path_str.clone(),
        source_lang: ctx.source_lang.clone(),
        target_lang: ctx.target_lang.clone(),
        segments: response
            .segments
            .into_iter()
            .map(|segment| TranslatedSegment {
                segment_id: segment.segment_id,
                start: segment.start,
                end: segment.end,
                source: segment.source,
                translation: segment.translation,
            })
            .collect(),
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::artifacts::SourceStats;
    use crate::cli::context::DB_NONE;
    use crate::commands::transcription::{
        GroupedSentenceSegmentCommandDto, GroupedSentenceTokenCommandDto,
    };
    use std::path::Path;

    fn cue(
        text: &str,
        semantic_sentence_id: usize,
        tokens: &[(&str, f64, f64)],
    ) -> GroupedSentenceSegmentCommandDto {
        GroupedSentenceSegmentCommandDto {
            segment: text.to_string(),
            semantic_sentence_id,
            start: tokens.first().map(|t| t.1).unwrap_or(0.0),
            end: tokens.last().map(|t| t.2).unwrap_or(0.0),
            tokens: tokens
                .iter()
                .map(|(text, start, end)| GroupedSentenceTokenCommandDto {
                    text: text.to_string(),
                    start: *start,
                    end: *end,
                })
                .collect(),
        }
    }

    /// Two display cues of one semantic sentence, so unit grouping and token
    /// survival are observable in the request.
    fn source() -> SourceArtifact {
        SourceArtifact {
            task_id: "t".into(),
            media_path: "sample.srt".into(),
            source_lang: "en".into(),
            target_lang: "zh-CN".into(),
            source_text: "NATO met at Canyon.".into(),
            segments: vec![
                cue("NATO met", 7, &[("NATO", 0.0, 0.5), ("met", 0.5, 1.0)]),
                cue("at Canyon.", 7, &[("at", 1.0, 1.2), ("Canyon.", 1.2, 2.0)]),
            ],
            stats: SourceStats::default(),
        }
    }

    fn common(name: &str) -> CommonArgs {
        CommonArgs {
            db: DB_NONE.to_string(),
            task: None,
            out: Some(PathBuf::from(format!("target/cli-llm-stage-tests/{name}"))),
            no_cache: false,
            source: Some("en".into()),
            target: Some("zh-CN".into()),
            asr_model: None,
            align_model: None,
            provider: None,
            chunk_seconds: None,
            models_dir: None,
            vocal_separation: false,
            no_vocal_separation: false,
            api_key: Some("test-key".into()),
            base_url: Some("https://example.invalid/v1".into()),
            model: Some("test-model".into()),
            concurrency: None,
            batch_size: None,
            terms: None,
            terms_group: None,
            style_guide: None,
            preset: None,
            beautify: false,
            no_beautify: false,
            json: false,
            quiet: false,
        }
    }

    async fn test_ctx(name: &str, style_guide: Option<&Path>, terms: Option<&Path>) -> CliContext {
        let mut args = common(name);
        args.style_guide = style_guide.map(Path::to_path_buf);
        args.terms = terms.map(Path::to_path_buf);
        CliContext::new(ContextRequest {
            verb: "test",
            common: &args,
            input_path: Some(Path::new("sample.srt")),
            require_llm: true,
        })
        .await
        .expect("test context resolves")
    }

    fn briefing_with(glossary: Vec<GlossaryEntry>, style_guide: &str) -> TerminologyBriefing {
        TerminologyBriefing {
            glossary,
            style_guide: style_guide.to_string(),
            windows: 1,
            end_reason: EndReason::Completed,
            skipped: false,
            skip_reason: None,
        }
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "voxtrans-cli-llm-stage-{}-{}.json",
            std::process::id(),
            name
        ));
        std::fs::write(&path, contents).expect("write temp file");
        path
    }

    /// The loader accepts what a `terminology` run writes (canonical
    /// snake_case) and the documented lightweight camelCase file; an empty
    /// file stays an empty (skipped) briefing rather than an error.
    #[test]
    fn briefing_loader_accepts_canonical_and_lightweight_files() {
        let canonical = temp_file(
            "canonical",
            r#"{"glossary":[{"source":"NATO","target":"北约","note":""}],"style_guide":"terse","windows":2,"end_reason":"early_exit","skipped":false}"#,
        );
        let briefing = load_briefing(&canonical).expect("canonical briefing");
        assert_eq!(briefing.glossary.len(), 1);
        assert_eq!(briefing.style_guide, "terse");
        assert_eq!(briefing.end_reason, EndReason::EarlyExit);
        assert!(!briefing.skipped);

        let light = temp_file(
            "light",
            r#"{"glossary":[{"source":"NATO","target":"北约"}],"styleGuide":"terse"}"#,
        );
        let briefing = load_briefing(&light).expect("lightweight briefing");
        assert_eq!(briefing.glossary.len(), 1);
        assert_eq!(briefing.style_guide, "terse");
        assert!(!briefing.skipped);

        let empty = temp_file("empty", r#"{"glossary":[],"styleGuide":""}"#);
        let briefing = load_briefing(&empty).expect("empty briefing");
        assert!(briefing.skipped);
        assert_eq!(briefing.end_reason, EndReason::Skipped);

        let err = load_briefing(&empty.join("does-not-exist.json"))
            .expect_err("missing file must be an error");
        assert!(err.contains("failed to read"), "unexpected error: {err}");
    }

    /// A request that reaches the service must keep the sentence grouping and
    /// the word tokens (dropping either degrades to per-cue translation), and
    /// the user glossary must win over the agent's row for the same source.
    #[tokio::test]
    async fn request_keeps_units_and_user_priority() {
        let terms = temp_file(
            "terms",
            r#"[{"source":"NATO","target":"北约-user","note":""}]"#,
        );
        let ctx = test_ctx("request", None, Some(&terms)).await;
        let briefing = briefing_with(
            vec![
                GlossaryEntry::new("NATO", "NATO-agent", ""),
                GlossaryEntry::new("Canyon", "Canyon-agent", "agent"),
                GlossaryEntry::new("ungrounded", "x", ""),
            ],
            "agent style",
        );

        let (request, stats) =
            build_translate_request(&ctx, &source(), Some(&briefing)).expect("request builds");

        assert_eq!(request.segments.len(), 2);
        assert_eq!(request.segments[0].sentence_id, 7);
        assert_eq!(request.segments[1].sentence_id, 7);
        assert_eq!(request.segments[0].tokens.len(), 2);
        assert_eq!(request.segments[0].tokens[1].text, "met");
        assert_eq!(request.segments[1].tokens[1].text, "Canyon.");

        let entries = &request.terminology_entries;
        assert_eq!(entries.len(), 2, "ungrounded agent rows must be dropped");
        assert!(
            entries
                .iter()
                .any(|e| e.source == "NATO" && e.target == "北约-user")
        );
        assert!(!entries.iter().any(|e| e.target == "NATO-agent"));
        assert!(
            entries
                .iter()
                .any(|e| e.source == "Canyon" && e.target == "Canyon-agent")
        );

        assert_eq!(stats.user_terms, 1);
        assert_eq!(stats.agent_terms, 3);
        assert_eq!(stats.merged_terms, 2);
        assert_eq!(request.style_guide, "agent style");
        assert!(request.unit_store.is_none(), "no --task => no resume store");
        assert!(request.llm_concurrency >= 1);

        std::fs::remove_file(&terms).ok();
    }

    /// `--style-guide` is the flag that wins over the briefing's guide.
    #[tokio::test]
    async fn explicit_style_guide_overrides_the_briefing() {
        let style = temp_file("style", "flag style");
        let ctx = test_ctx("style", Some(&style), None).await;
        let briefing = briefing_with(vec![], "agent style");

        let (request, stats) =
            build_translate_request(&ctx, &source(), Some(&briefing)).expect("request builds");
        assert_eq!(request.style_guide, "flag style");
        assert_eq!(stats.style_guide_chars, "flag style".chars().count());

        std::fs::remove_file(&style).ok();
    }

    /// An injected briefing is written to `<out>/briefing.json` unchanged (the
    /// caller reports it as a cache hit) and the plan for a semantic-sentence
    /// source covers both slots with one unit, offline.
    #[tokio::test]
    async fn injected_briefing_round_trips_and_dry_run_plans_units() {
        let inject = temp_file(
            "inject",
            r#"{"glossary":[{"source":"NATO","target":"北约"}],"styleGuide":"terse"}"#,
        );
        let ctx = test_ctx("inject", None, None).await;

        let briefing = terminology_stage(&ctx, &source(), Some(&inject))
            .await
            .expect("injected briefing");
        assert_eq!(briefing.glossary.len(), 1);

        let written = ctx.out_path(BRIEFING_JSON);
        assert!(written.is_file(), "briefing.json must be written");
        let reloaded = load_briefing(&written).expect("written briefing reloads");
        assert_eq!(reloaded.glossary[0].target, "北约");

        let (plan, stats) = plan_translate(&ctx, &source(), None).expect("dry-run plan");
        assert_eq!(
            plan.unit_total, 1,
            "both cues belong to one semantic sentence"
        );
        assert_eq!(plan.window_total, 1);
        assert_eq!(plan.windows[0].unit_ids, vec![1]);
        assert_eq!(plan.windows[0].slot_ids, vec![1, 2]);
        assert!(plan.windows[0].prompt.contains("NATO met"));
        assert_eq!(stats.user_terms, 0);

        std::fs::remove_file(&inject).ok();
    }
}
