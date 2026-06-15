use std::path::{Path, PathBuf};
use std::time::Instant;

use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions as AsrTranscribeOptions};
use qwen_forced_aligner_rs::{
    AlignRequest, AudioInput, DeviceRequest, ForcedAlignItem, ForcedAlignResult,
    ModelOptions, TextInput, load_model,
};
use voxtrans_core::subtitle::{alignment::align_text_to_timestamps, segmenter::WordToken};

pub(super) struct AsrAlignRequest {
    pub(super) audio_path: PathBuf,
    pub(super) source_lang: String,
    pub(super) asr_model: String,
    pub(super) align_model: String,
    pub(super) provider: String,
    pub(super) chunk_target_seconds: u32,
    pub(super) model_dir: Option<PathBuf>,
    /// Previously computed ASR transcripts keyed by segment index.
    /// When non-empty, ASR is skipped for these segments.
    pub(super) precomputed_asr: Vec<(usize, String)>,
    /// Previously computed alignment results keyed by segment index.
    /// When non-empty, alignment is skipped for these segments.
    pub(super) precomputed_alignment: Vec<(usize, ForcedAlignResult)>,
}

pub(super) struct AsrAlignOutput {
    pub(super) words: Vec<WordToken>,
    pub(super) text: String,
    pub(super) aligned_text: String,
    pub(super) segment_summaries: Vec<voxtrans_core::SegmentSummary>,
    pub(super) audio_duration_sec: f64,
    pub(super) vad_elapsed_sec: f64,
    pub(super) vad_speech_segments: Vec<(f64, f64)>,
    pub(super) transcribe_elapsed_sec: f64,
    pub(super) timing: AsrAlignTiming,
    pub(super) execution_provider: String,
    /// ASR transcripts that were freshly computed (not from precomputed cache).
    /// `Vec<(segment_index, text)>`.
    pub(super) new_asr_results: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct AsrAlignTiming {
    pub(super) prepare_elapsed_sec: f64,
    pub(super) vad_elapsed_sec: f64,
    pub(super) temp_wav_write_sec: f64,
    pub(super) asr_load_sec: f64,
    pub(super) asr_transcribe_sec: f64,
    pub(super) qwen_load_sec: f64,
    pub(super) qwen_align_sec: f64,
    pub(super) punctuation_map_sec: f64,
    pub(super) total_elapsed_sec: f64,
}

pub(super) fn transcribe_with_asr_and_qwen<F>(
    request: AsrAlignRequest,
    mut on_progress: F,
) -> Result<AsrAlignOutput, String>
where
    F: FnMut(TranscribeProgressStage, usize, usize, Option<FreshSegmentResult>),
{
    let started_at = Instant::now();
    let chunk_target_seconds = request.chunk_target_seconds.clamp(30, 60) as f64;
    let prepare_started_at = Instant::now();
    let prepared =
        voxtrans_core::prepare_audio_segments_for_asr(&request.audio_path, chunk_target_seconds)
            .map_err(|err| err.to_string())?;
    let mut timing = AsrAlignTiming {
        prepare_elapsed_sec: prepare_started_at.elapsed().as_secs_f64(),
        vad_elapsed_sec: prepared.vad_elapsed_sec,
        ..AsrAlignTiming::default()
    };

    let asr_model_dir = request
        .model_dir
        .unwrap_or_else(|| crate::services::model::resolve_asr_model_dir(&request.asr_model));
    let aligner_model_dir = crate::services::model::resolve_aligner_model_dir(&request.align_model);
    let device = provider_to_device(&request.provider)?;
    let language = asr_language(&request.source_lang)?;
    let aligner_language = qwen_language(&request.source_lang)?;
    let total_segments = prepared.segment_summaries.len();
    let sample_len = prepared.mono_samples.len();

    let precomputed_asr_map: std::collections::HashMap<usize, String> =
        request.precomputed_asr.into_iter().collect();
    let precomputed_alignment_map: std::collections::HashMap<usize, ForcedAlignResult> =
        request.precomputed_alignment.into_iter().collect();

    let segment_transcripts = {
        let load_started_at = Instant::now();
        let transcriber =
            AsrInference::load(&asr_model_dir, device.asr_device.clone()).map_err(|err| {
                format!(
                    "failed to load ASR model from {}: {err:#}",
                    asr_model_dir.display()
                )
            })?;
        timing.asr_load_sec = load_started_at.elapsed().as_secs_f64();

        let mut transcripts = Vec::new();
        let mut new_results = Vec::new();
        for segment in &prepared.segment_summaries {
            let start_index = ((segment.start_sec * voxtrans_core::TARGET_SAMPLE_RATE as f64)
                .floor() as usize)
                .min(sample_len);
            let end_index = ((segment.end_sec * voxtrans_core::TARGET_SAMPLE_RATE as f64).ceil()
                as usize)
                .min(sample_len);
            if end_index <= start_index {
                on_progress(TranscribeProgressStage::Asr, segment.index, total_segments, None);
                continue;
            }

            // Check if this segment's ASR was precomputed
            if let Some(cached_text) = precomputed_asr_map.get(&segment.index) {
                if !cached_text.is_empty() {
                    let segment_samples = &prepared.mono_samples[start_index..end_index];
                    let wav_started_at = Instant::now();
                    let wav = TemporaryWav::write(segment_samples)
                        .map_err(|err| format!("failed to write temporary wav: {err}"))?;
                    timing.temp_wav_write_sec += wav_started_at.elapsed().as_secs_f64();
                    transcripts.push(SegmentTranscript {
                        start_sec: segment.start_sec,
                        segment_index: segment.index,
                        wav,
                        text: cached_text.clone(),
                    });
                }
                on_progress(TranscribeProgressStage::Asr, segment.index, total_segments, None);
                continue;
            }

            let segment_samples = &prepared.mono_samples[start_index..end_index];
            let wav_started_at = Instant::now();
            let wav = TemporaryWav::write(segment_samples)
                .map_err(|err| format!("failed to write temporary wav: {err}"))?;
            timing.temp_wav_write_sec += wav_started_at.elapsed().as_secs_f64();
            let transcribe_started_at = Instant::now();
            let mut options = AsrTranscribeOptions::default();
            options.language = Some(language.clone());
            let report = transcriber
                .transcribe_samples(segment_samples, options)
                .map_err(|err| format!("asr transcription failed: {err:#}"))?;
            timing.asr_transcribe_sec += transcribe_started_at.elapsed().as_secs_f64();
            let text = clean_asr_text(&report.text);
            if text.is_empty() {
                on_progress(TranscribeProgressStage::Asr, segment.index, total_segments, None);
                continue;
            }
            on_progress(
                TranscribeProgressStage::Asr,
                segment.index,
                total_segments,
                Some(FreshSegmentResult::Asr {
                    segment_index: segment.index,
                    text: text.clone(),
                }),
            );
            new_results.push((segment.index, text.clone()));
            transcripts.push(SegmentTranscript {
                start_sec: segment.start_sec,
                segment_index: segment.index,
                wav,
                text,
            });
        }
        SegmentTranscriptsWithNew {
            transcripts,
            new_results,
        }
    };

    let transcript_text = debug_text_from_transcripts(&segment_transcripts.transcripts);
    let (words, aligned_text) = if segment_transcripts.transcripts.is_empty() {
        (Vec::new(), String::new())
    } else {
        let load_started_at = Instant::now();
        let aligner = load_model(
            &aligner_model_dir,
            ModelOptions {
                device: device.qwen_device,
            },
        )
        .map_err(|err| {
            format!(
                "failed to load Qwen aligner model from {}: {err:#}",
                aligner_model_dir.display()
            )
        })?;
        timing.qwen_load_sec = load_started_at.elapsed().as_secs_f64();

        let align_started_at = Instant::now();
        let results = align_segments(
            &aligner,
            &aligner_language,
            &segment_transcripts.transcripts,
            precomputed_alignment_map,
            &mut on_progress,
        )
        .map_err(|err| format!("qwen alignment failed: {err:#}"))?;
        timing.qwen_align_sec = align_started_at.elapsed().as_secs_f64();
        let segment_starts = segment_transcripts
            .transcripts
            .iter()
            .map(|segment| segment.start_sec)
            .collect::<Vec<_>>();
        let segment_texts = segment_transcripts
            .transcripts
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>();
        let punctuation_started_at = Instant::now();
        let output = words_from_alignment_results(&segment_starts, &segment_texts, results);
        timing.punctuation_map_sec = punctuation_started_at.elapsed().as_secs_f64();
        (output.words, output.aligned_text)
    };
    timing.total_elapsed_sec = started_at.elapsed().as_secs_f64();

    Ok(AsrAlignOutput {
        words,
        text: transcript_text,
        aligned_text,
        segment_summaries: prepared.segment_summaries,
        audio_duration_sec: prepared.audio_duration_sec,
        vad_elapsed_sec: prepared.vad_elapsed_sec,
        vad_speech_segments: prepared.vad_speech_segments,
        transcribe_elapsed_sec: timing.total_elapsed_sec,
        timing,
        execution_provider: device.label,
        new_asr_results: segment_transcripts.new_results,
    })
}

struct SegmentTranscript {
    start_sec: f64,
    segment_index: usize,
    wav: TemporaryWav,
    text: String,
}

struct SegmentTranscriptsWithNew {
    transcripts: Vec<SegmentTranscript>,
    new_results: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscribeProgressStage {
    Asr,
    Align,
}

/// Payload for a freshly computed segment result that should be persisted.
#[derive(Debug, Clone)]
pub(crate) enum FreshSegmentResult {
    Asr { segment_index: usize, text: String },
    Align {
        segment_index: usize,
        items: Vec<ForcedAlignItem>,
    },
}

struct RuntimeDevice {
    asr_device: AsrBackend,
    qwen_device: DeviceRequest,
    label: String,
}

struct AlignmentWordsOutput {
    words: Vec<WordToken>,
    aligned_text: String,
}

fn align_segments(
    aligner: &qwen_forced_aligner_rs::Qwen3ForcedAligner,
    aligner_language: &str,
    segment_transcripts: &[SegmentTranscript],
    precomputed_alignment: std::collections::HashMap<usize, ForcedAlignResult>,
    on_progress: &mut impl FnMut(TranscribeProgressStage, usize, usize, Option<FreshSegmentResult>),
) -> Result<Vec<ForcedAlignResult>, String> {
    let total = segment_transcripts.len();
    let mut results = Vec::with_capacity(total);
    for (index, segment) in segment_transcripts.iter().enumerate() {
        let current = index + 1;
        if let Some(cached) = precomputed_alignment.get(&segment.segment_index) {
            on_progress(TranscribeProgressStage::Align, current, total, None);
            results.push(cached.clone());
            continue;
        }
        let result = aligner
            .align(AlignRequest::new(
                AudioInput::Path(segment.wav.path.clone()),
                TextInput::Text(segment.text.clone()),
                aligner_language.to_string(),
            ))
            .map_err(|err| format!("failed align request {current}: {err:#}"))?;
        on_progress(
            TranscribeProgressStage::Align,
            current,
            total,
            Some(FreshSegmentResult::Align {
                segment_index: segment.segment_index,
                items: result.items.clone(),
            }),
        );
        results.push(result);
    }
    Ok(results)
}

fn provider_to_device(provider: &str) -> Result<RuntimeDevice, String> {
    let normalized = provider.trim().to_ascii_lowercase();
    if normalized == "cpu" {
        return Ok(RuntimeDevice {
            asr_device: AsrBackend::Cpu,
            qwen_device: DeviceRequest::Cpu,
            label: "cpu".to_string(),
        });
    }

    #[cfg(feature = "cuda")]
    {
        Ok(RuntimeDevice {
            asr_device: AsrBackend::Cuda,
            qwen_device: DeviceRequest::Cuda(0),
            label: "cuda".to_string(),
        })
    }
    #[cfg(not(feature = "cuda"))]
    {
        Err("CUDA provider requested but this build was not compiled with CUDA support".to_string())
    }
}

fn asr_language(source_lang: &str) -> Result<String, String> {
    let normalized = source_lang.trim().to_ascii_lowercase();
    let language = match normalized.as_str() {
        "en" | "en-us" | "english" => "english",
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "mandarin" => "chinese",
        "ja" | "ja-jp" | "japanese" => "japanese",
        "ko" | "ko-kr" | "korean" => "korean",
        "fr" | "fr-fr" | "french" => "french",
        "de" | "de-de" | "german" => "german",
        "it" | "it-it" | "italian" => "italian",
        "es" | "es-es" | "spanish" => "spanish",
        "pt" | "pt-pt" | "pt-br" | "portuguese" => "portuguese",
        "yue" | "yue-hk" | "zh-yue" | "cantonese" | "粤语" | "廣東話" | "广东话" => {
            "cantonese"
        }
        _ => return Err(format!("unsupported source language: {source_lang}")),
    };
    Ok(language.to_string())
}

fn qwen_language(source_lang: &str) -> Result<String, String> {
    let normalized = source_lang.trim().to_ascii_lowercase();
    let language = match normalized.as_str() {
        "en" | "en-us" | "english" => "English",
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "mandarin" => "Chinese",
        "ja" | "ja-jp" | "japanese" => "Japanese",
        "ko" | "ko-kr" | "korean" => "Korean",
        "fr" | "french" => "French",
        "de" | "german" => "German",
        "es" | "spanish" => "Spanish",
        "it" | "italian" => "Italian",
        "pt" | "portuguese" => "Portuguese",
        "yue" | "yue-hk" | "zh-yue" | "cantonese" | "粤语" | "廣東話" | "广东话" => {
            "Cantonese"
        }
        _ => return Err(format!("unsupported source language: {source_lang}")),
    };
    Ok(language.to_string())
}

fn clean_asr_text(raw: &str) -> String {
    let mut text = raw.trim();
    for marker in ["<asr_text>", "asr_text>"] {
        if let Some((_, rest)) = text.split_once(marker) {
            text = rest.trim();
        }
    }
    text.trim_matches(['<', '>']).trim().to_string()
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn words_from_alignment_results(
    segment_starts: &[f64],
    segment_texts: &[&str],
    results: Vec<ForcedAlignResult>,
) -> AlignmentWordsOutput {
    let mut words = Vec::new();
    let mut aligned_text = String::new();
    for ((segment_start, transcript_text), result) in
        segment_starts.iter().zip(segment_texts.iter()).zip(results)
    {
        let mut segment_words = Vec::new();
        for item in result.items {
            let word = item.text.trim();
            if word.is_empty() {
                continue;
            }
            segment_words.push(WordToken {
                start: round_millis(segment_start + item.start_time.max(0.0)),
                end: round_millis(segment_start + item.end_time.max(item.start_time)),
                word: word.to_string(),
            });
        }
        let aligned_segment_text = debug_text_from_words(&segment_words);
        if !aligned_segment_text.is_empty() {
            append_debug_piece(&mut aligned_text, &aligned_segment_text);
        }
        words.extend(attach_transcript_punctuation(
            transcript_text,
            &segment_words,
        ));
    }
    AlignmentWordsOutput {
        words,
        aligned_text,
    }
}

fn attach_transcript_punctuation(
    transcript_text: &str,
    aligned_words: &[WordToken],
) -> Vec<WordToken> {
    if transcript_text.trim().is_empty() || aligned_words.is_empty() {
        return aligned_words.to_vec();
    }

    let mapped = align_text_to_timestamps(transcript_text, aligned_words);
    if mapped.len() == aligned_words.len() {
        mapped
    } else {
        aligned_words.to_vec()
    }
}

fn debug_text_from_transcripts(segments: &[SegmentTranscript]) -> String {
    let mut text = String::new();
    for segment in segments {
        append_debug_piece(&mut text, segment.text.trim());
    }
    text
}

fn debug_text_from_words(words: &[WordToken]) -> String {
    let mut text = String::new();
    for word in words {
        append_debug_piece(&mut text, word.word.trim());
    }
    text
}

fn append_debug_piece(text: &mut String, piece: &str) {
    if piece.is_empty() {
        return;
    }
    if should_insert_debug_space(text, piece) {
        text.push(' ');
    }
    text.push_str(piece);
}

fn should_insert_debug_space(current: &str, next: &str) -> bool {
    let Some(previous) = current.chars().last() else {
        return false;
    };
    let Some(first) = next.chars().next() else {
        return false;
    };
    previous.is_alphanumeric()
        && first.is_alphanumeric()
        && !is_compact_script_char(previous)
        && !is_compact_script_char(first)
        || previous.is_ascii_punctuation()
            && first.is_alphanumeric()
            && !is_compact_script_char(first)
}

fn is_compact_script_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
    )
}

struct TemporaryWav {
    path: PathBuf,
}

impl TemporaryWav {
    fn write(samples: &[f32]) -> Result<Self, hound::Error> {
        let path = temp_wav_path();
        write_wav_mono_16k(&path, samples)?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_wav_mono_16k(path: &Path, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: voxtrans_core::TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer.write_sample(value)?;
    }
    writer.finalize()?;
    Ok(())
}

fn temp_wav_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("voxtrans_asr_align_{pid}_{nanos}.wav"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use qwen_forced_aligner_rs::{ForcedAlignItem, ForcedAlignResult};

    #[test]
    fn alignment_results_are_offset_to_original_timeline() {
        let output = words_from_alignment_results(
            &[12.5],
            &["hello"],
            vec![ForcedAlignResult {
                items: vec![
                    ForcedAlignItem {
                        text: " hello ".to_string(),
                        start_time: 0.1234,
                        end_time: 0.4567,
                    },
                    ForcedAlignItem {
                        text: "".to_string(),
                        start_time: 0.5,
                        end_time: 0.6,
                    },
                ],
                output_ids: Vec::new(),
                raw_timestamp_ms: Vec::new(),
                fixed_timestamp_ms: Vec::new(),
            }],
        );
        assert_eq!(output.aligned_text, "hello");
        let words = output.words;

        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "hello");
        assert_eq!(words[0].start, 12.623);
        assert_eq!(words[0].end, 12.957);
    }

    #[test]
    fn alignment_results_attach_transcript_punctuation() {
        let output = words_from_alignment_results(
            &[3.0],
            &["Hello, world!"],
            vec![ForcedAlignResult {
                items: vec![
                    ForcedAlignItem {
                        text: "Hello".to_string(),
                        start_time: 0.0,
                        end_time: 0.4,
                    },
                    ForcedAlignItem {
                        text: "world".to_string(),
                        start_time: 0.5,
                        end_time: 0.9,
                    },
                ],
                output_ids: Vec::new(),
                raw_timestamp_ms: Vec::new(),
                fixed_timestamp_ms: Vec::new(),
            }],
        );
        assert_eq!(output.aligned_text, "Hello world");
        let words = output.words;

        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Hello,");
        assert_eq!(words[1].word, "world!");
        assert_eq!(words[0].start, 3.0);
        assert_eq!(words[1].end, 3.9);
    }

    #[test]
    fn alignment_results_expose_unpunctuated_aligned_text() {
        let output = words_from_alignment_results(
            &[0.0],
            &["你好,世界。"],
            vec![ForcedAlignResult {
                items: vec![
                    ForcedAlignItem {
                        text: "你".to_string(),
                        start_time: 0.0,
                        end_time: 0.1,
                    },
                    ForcedAlignItem {
                        text: "好".to_string(),
                        start_time: 0.1,
                        end_time: 0.2,
                    },
                    ForcedAlignItem {
                        text: "世".to_string(),
                        start_time: 0.2,
                        end_time: 0.3,
                    },
                    ForcedAlignItem {
                        text: "界".to_string(),
                        start_time: 0.3,
                        end_time: 0.4,
                    },
                ],
                output_ids: Vec::new(),
                raw_timestamp_ms: Vec::new(),
                fixed_timestamp_ms: Vec::new(),
            }],
        );

        assert_eq!(output.aligned_text, "你好世界");
        assert_eq!(output.words[1].word, "好,");
        assert_eq!(output.words[3].word, "界。");
    }

    #[test]
    fn supported_source_languages_map_to_asr_and_qwen() {
        let cases = [
            ("en", "english", "English"),
            ("zh", "chinese", "Chinese"),
            ("ja", "japanese", "Japanese"),
            ("ko", "korean", "Korean"),
            ("fr", "french", "French"),
            ("de", "german", "German"),
            ("it", "italian", "Italian"),
            ("es", "spanish", "Spanish"),
            ("pt", "portuguese", "Portuguese"),
            ("yue", "cantonese", "Cantonese"),
            ("Cantonese", "cantonese", "Cantonese"),
            ("广东话", "cantonese", "Cantonese"),
        ];

        for (source, asr, qwen) in cases {
            assert_eq!(asr_language(source).as_deref(), Ok(asr));
            assert_eq!(qwen_language(source).as_deref(), Ok(qwen));
        }
    }

    #[test]
    fn unsupported_source_languages_are_rejected() {
        for source in ["", "auto", "ru", "ar", "vi", "nl", "pl", "el"] {
            assert!(asr_language(source).is_err());
            assert!(qwen_language(source).is_err());
        }
    }

    #[test]
    fn clean_asr_text_removes_model_protocol_marker() {
        assert_eq!(
            clean_asr_text("asr_text>Everybody has problems, even you."),
            "Everybody has problems, even you."
        );
        assert_eq!(
            clean_asr_text("language English<asr_text>God bless."),
            "God bless."
        );
        assert_eq!(clean_asr_text("  Plain text.  "), "Plain text.");
    }
}
