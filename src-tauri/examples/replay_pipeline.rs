//! Replay step2 + beautify + translation from a saved ASR dump.
//! Not invoked by the app; used to evaluate local pipeline changes
//! against an existing transcription without re-running ASR.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use voxtrans::commands::transcription::{
    BuildSourceSentencesCommandRequest, WordTokenCommandDto, build_source_sentences_with_progress,
};
use voxtrans::db::open_pool_at;
use voxtrans::db::store::TaskStore;
use voxtrans::domain::pipeline::UnitStore;
use voxtrans::domain::task::adapters::workspace_subtitle_segments_from_step2_segments;
use voxtrans::services::subtitle_beautify::beautify_workspace_segments;
use voxtrans::services::subtitle_srt::{
    write_variants_to_directory, ExportSrtItem, SubtitleSrtSegment,
};
use voxtrans::services::translation::{
    build_translation_layer_with_progress, BuildTranslationLayerRequest, TranslationSegmentInput,
    TranslationToken,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySettings {
    task_id: String,
    source_lang: String,
    target_lang: String,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    llm_concurrency: u32,
    subtitle_length_preset: String,
    enable_subtitle_beautify: bool,
    asr_json: String,
    db_path: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrDump {
    words: Vec<WordTokenCommandDto>,
    vad_speech_segments: Vec<(f64, f64)>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("replay failed: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let settings_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\Users\ADMIN\AppData\Local\Temp\vt-replay-settings.json".into());
    let settings: ReplaySettings = serde_json::from_str(
        &fs::read_to_string(&settings_path).map_err(|e| format!("read settings: {e}"))?,
    )
    .map_err(|e| format!("parse settings: {e}"))?;

    let asr: AsrDump = serde_json::from_str(
        &fs::read_to_string(&settings.asr_json).map_err(|e| format!("read asr: {e}"))?,
    )
    .map_err(|e| format!("parse asr: {e}"))?;
    if asr.words.is_empty() {
        return Err("asr dump has no words".into());
    }

    let out_dir = PathBuf::from(r"C:\Users\ADMIN\AppData\Local\VoxTrans\output\video_replay-mfkge5");
    fs::create_dir_all(out_dir.join("artifacts").join("logs")).map_err(|e| e.to_string())?;
    let media_path = out_dir.join("video.mp4");
    let media_path_str = media_path.to_string_lossy().to_string();
    let status_path = out_dir.join("replay_status.txt");

    eprintln!(
        "replay start task={} words={} model={} beautify={}",
        settings.task_id,
        asr.words.len(),
        settings.translate_model,
        settings.enable_subtitle_beautify
    );
    write_status(&status_path, "step2 starting");

    let step2_t = Instant::now();
    let step2 = build_source_sentences_with_progress(
        BuildSourceSentencesCommandRequest {
            task_id: settings.task_id.clone(),
            audio_path: media_path_str.clone(),
            source_lang: settings.source_lang.clone(),
            subtitle_length_preset: settings.subtitle_length_preset.clone(),
            words: asr.words,
            vad_speech_segments: asr.vad_speech_segments,
        },
        None,
    )
    .await?;
    eprintln!(
        "step2 done cues={} in {:.1}s",
        step2.sentence_total,
        step2_t.elapsed().as_secs_f64()
    );

    let mut workspace = workspace_subtitle_segments_from_step2_segments(&step2.segments);
    if settings.enable_subtitle_beautify {
        beautify_workspace_segments(
            &mut workspace,
            &settings.subtitle_length_preset,
            &settings.target_lang,
        );
        eprintln!("source beautify cues={}", workspace.len());
    }

    let source_only = std::env::args().any(|a| a == "--source-only");
    let source_srt: Vec<SubtitleSrtSegment> = workspace
        .iter()
        .map(|seg| SubtitleSrtSegment {
            start_ms: seg.start_ms,
            end_ms: seg.end_ms,
            source_text: seg.source_text.clone(),
            translated_text: seg.translated_text.clone(),
        })
        .collect();
    write_variants_to_directory(&out_dir, &source_srt, &[ExportSrtItem::Source])?;
    eprintln!("wrote {}", out_dir.join("src.srt").display());
    if source_only {
        write_status(
            &status_path,
            &format!("source-only cues={}", workspace.len()),
        );
        return Ok(());
    }

    let pool = open_pool_at(Path::new(&settings.db_path)).await?;
    let store = TaskStore::new(pool);
    let unit_store = UnitStore::new(&store, &settings.task_id);

    let translate_inputs: Vec<TranslationSegmentInput> = workspace
        .iter()
        .map(|seg| TranslationSegmentInput {
            segment: seg.source_text.clone(),
            start: seg.start_ms as f64 / 1000.0,
            end: seg.end_ms as f64 / 1000.0,
            tokens: seg
                .source_words
                .iter()
                .map(|w| TranslationToken {
                    text: w.word.clone(),
                    start: w.start_ms as f64 / 1000.0,
                    end: w.end_ms as f64 / 1000.0,
                })
                .collect(),
        })
        .collect();

    let done_flag = Arc::new(AtomicUsize::new(0));
    let status_for_cb = status_path.clone();
    let on_progress = {
        let done_flag = done_flag.clone();
        Arc::new(move |progress: voxtrans::services::translation::TranslationProgress| {
            done_flag.store(progress.done, Ordering::Relaxed);
            let line = format!("translating {}/{}", progress.done, progress.total);
            eprintln!("{line}");
            write_status(&status_for_cb, &line);
        }) as Arc<dyn Fn(voxtrans::services::translation::TranslationProgress) + Send + Sync>
    };

    write_status(&status_path, "translation starting");
    let tr_t = Instant::now();
    let translated = build_translation_layer_with_progress(
        BuildTranslationLayerRequest {
            task_id: settings.task_id.clone(),
            media_path: media_path_str,
            source_lang: settings.source_lang.clone(),
            target_lang: settings.target_lang.clone(),
            segments: translate_inputs,
            style_guide: String::new(),
            terminology_entries: Vec::new(),
            translate_api_key: settings.translate_api_key.clone(),
            translate_base_url: settings.translate_base_url.clone(),
            translate_model: settings.translate_model.clone(),
            llm_concurrency: settings.llm_concurrency,
            batch_size: 20,
            unit_store: Some(unit_store),
        },
        Some(on_progress),
    )
    .await?;
    eprintln!(
        "translation done batches={} segments={} in {:.1}s",
        translated.batch_total,
        translated.segment_total,
        tr_t.elapsed().as_secs_f64()
    );

    let mut by_id = std::collections::HashMap::new();
    for seg in &translated.segments {
        by_id.insert(seg.segment_id, seg.translation.clone());
    }
    for (i, seg) in workspace.iter_mut().enumerate() {
        if let Some(text) = by_id.get(&(i + 1)) {
            seg.translated_text = text.clone();
        }
    }
    if settings.enable_subtitle_beautify {
        beautify_workspace_segments(
            &mut workspace,
            &settings.subtitle_length_preset,
            &settings.target_lang,
        );
        eprintln!("target beautify cues={}", workspace.len());
    }

    let srt_segments: Vec<SubtitleSrtSegment> = workspace
        .iter()
        .map(|seg| SubtitleSrtSegment {
            start_ms: seg.start_ms,
            end_ms: seg.end_ms,
            source_text: seg.source_text.clone(),
            translated_text: seg.translated_text.clone(),
        })
        .collect();
    let written = write_variants_to_directory(
        &out_dir,
        &srt_segments,
        &[
            ExportSrtItem::Source,
            ExportSrtItem::Target,
            ExportSrtItem::BilingualSourceFirst,
            ExportSrtItem::BilingualTargetFirst,
        ],
    )?;
    for path in &written {
        eprintln!("wrote {path}");
    }
    write_status(
        &status_path,
        &format!("done cues={} files={}", srt_segments.len(), written.len()),
    );
    Ok(())
}

fn write_status(path: &Path, line: &str) {
    let _ = fs::write(path, format!("{line}\n"));
}
