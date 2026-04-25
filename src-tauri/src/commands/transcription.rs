#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceCommandDto {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunkCommandDto {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecisionCommandDto {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: crate::services::transcription::BoundaryDecisionKind,
    pub llm_decision: crate::services::transcription::BoundaryDecisionKind,
    pub final_decision: crate::services::transcription::BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildSourceSentencesCommandRequest {
    pub task_id: String,
    pub audio_path: String,
    pub source_lang: String,
    pub words: Vec<WordTokenCommandDto>,
    #[serde(default = "default_subtitle_max_words_per_segment")]
    pub subtitle_max_words_per_segment: u32,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildSourceSentencesCommandResponse {
    pub hard_split_gap_ms: u64,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunkCommandDto>,
    pub boundaries: Vec<BoundaryDecisionCommandDto>,
    pub translation_sentences: Vec<SourceSentenceCommandDto>,
    pub segments: Vec<GroupedSentenceSegmentCommandDto>,
    pub srt: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupedSentenceTokenCommandDto {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GroupedSentenceSegmentCommandDto {
    pub segment: String,
    pub start: f64,
    pub end: f64,
    pub tokens: Vec<GroupedSentenceTokenCommandDto>,
}

#[tauri::command]
pub async fn build_source_sentences(
    request: BuildSourceSentencesCommandRequest,
) -> Result<BuildSourceSentencesCommandResponse, String> {
    build_source_sentences_with_progress(request, None).await
}

pub async fn build_source_sentences_with_progress(
    request: BuildSourceSentencesCommandRequest,
    on_progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> Result<BuildSourceSentencesCommandResponse, String> {
    let original_words = request.words.clone();
    let step2 = crate::services::transcription::build_source_sentences_from_words_with_progress(
        crate::services::transcription::SentenceBoundaryRequest {
            task_id: request.task_id,
            media_path: request.audio_path,
            source_lang: request.source_lang,
            words: request.words.into_iter().map(to_service_word).collect(),
            subtitle_max_words_per_segment: request.subtitle_max_words_per_segment,
            translate_api_key: request.translate_api_key,
            translate_base_url: request.translate_base_url,
            translate_model: request.translate_model,
            llm_concurrency: request.llm_concurrency,
        },
        on_progress,
    )
    .await?;
    let srt = crate::services::transcription::source_sentences_to_srt(&step2);
    let translation_sentences = step2
        .translation_sentences
        .into_iter()
        .map(|sentence| SourceSentenceCommandDto {
            sentence_id: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text,
            word_start: sentence.word_start,
            word_end: sentence.word_end,
            chunk_start: sentence.chunk_start,
            chunk_end: sentence.chunk_end,
        })
        .collect::<Vec<_>>();
    let segments = build_grouped_sentence_segments(&original_words, &translation_sentences);
    Ok(BuildSourceSentencesCommandResponse {
        hard_split_gap_ms: step2.hard_split_gap_ms,
        micro_chunk_total: step2.micro_chunk_total,
        boundary_total: step2.boundary_total,
        sentence_total: step2.sentence_total,
        micro_chunks: step2
            .micro_chunks
            .iter()
            .cloned()
            .map(|chunk| MicroChunkCommandDto {
                chunk_id: chunk.chunk_id,
                start_ms: chunk.start_ms,
                end_ms: chunk.end_ms,
                text: chunk.text,
                word_start: chunk.word_start,
                word_end: chunk.word_end,
                gap_before_ms: chunk.gap_before_ms,
                gap_after_ms: chunk.gap_after_ms,
                hard_split_before: chunk.hard_split_before,
                hard_split_after: chunk.hard_split_after,
            })
            .collect(),
        boundaries: step2
            .boundaries
            .into_iter()
            .map(|boundary| BoundaryDecisionCommandDto {
                left_chunk_id: boundary.left_chunk_id,
                right_chunk_id: boundary.right_chunk_id,
                gap_ms: boundary.gap_ms,
                rule_decision: boundary.rule_decision,
                llm_decision: boundary.llm_decision,
                final_decision: boundary.final_decision,
                confidence: boundary.confidence,
                reason_tag: boundary.reason_tag,
            })
            .collect(),
        translation_sentences,
        segments,
        srt,
    })
}

fn build_grouped_sentence_segments(
    words: &[WordTokenCommandDto],
    sentences: &[SourceSentenceCommandDto],
) -> Vec<GroupedSentenceSegmentCommandDto> {
    let mut out = Vec::<GroupedSentenceSegmentCommandDto>::new();
    if words.is_empty() {
        return out;
    }

    for sentence in sentences {
        if sentence.word_start >= words.len() {
            continue;
        }
        let end = sentence.word_end.min(words.len() - 1);
        if end < sentence.word_start {
            continue;
        }
        let sentence_words = &words[sentence.word_start..=end];
        let start = sentence_words
            .first()
            .map(|token| token.start)
            .unwrap_or(0.0);
        let end = sentence_words
            .last()
            .map(|token| token.end)
            .unwrap_or(start);
        let tokens = sentence_words
            .iter()
            .map(|token| GroupedSentenceTokenCommandDto {
                text: token.word.clone(),
                start: token.start,
                end: token.end,
            })
            .collect::<Vec<_>>();
        let segment = if sentence.text.trim().is_empty() {
            fallback_segment_text_from_tokens(&tokens)
        } else {
            sentence.text.clone()
        };
        out.push(GroupedSentenceSegmentCommandDto {
            segment,
            start,
            end,
            tokens,
        });
    }

    out
}

fn fallback_segment_text_from_tokens(tokens: &[GroupedSentenceTokenCommandDto]) -> String {
    let mut out = String::new();
    let mut prev_word_like = false;

    for token in tokens {
        let piece = token.text.trim();
        if piece.is_empty() {
            continue;
        }

        let next_word_like = token_has_spacing_word(piece);
        let next_starts_with_joiner = starts_with_joiner(piece);
        let prev_ends_with_spacing_punctuation = out
            .chars()
            .rev()
            .find(|ch| !ch.is_whitespace())
            .map(is_spacing_punctuation)
            .unwrap_or(false);

        if !out.is_empty()
            && ((prev_word_like && next_word_like && !next_starts_with_joiner)
                || (prev_ends_with_spacing_punctuation && next_word_like))
        {
            out.push(' ');
        }

        out.push_str(piece);
        prev_word_like = next_word_like;
    }

    out
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn starts_with_joiner(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|ch| matches!(ch, '\'' | '’'))
        .unwrap_or(false)
}

fn is_spacing_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ',' | '.' | '!' | '?' | ':' | ';' | '，' | '。' | '！' | '？' | '：' | '；'
    )
}

fn is_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}

fn to_service_word(word: WordTokenCommandDto) -> crate::services::transcribe::WordTokenDto {
    crate::services::transcribe::WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

fn default_llm_concurrency() -> u32 {
    4
}

fn default_subtitle_max_words_per_segment() -> u32 {
    20
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AsrArtifactForSentenceCli {
    task_id: String,
    media_path: String,
    source_lang: String,
    words: Vec<WordTokenCommandDto>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum AsrArtifactForSentenceCliInput {
    Flat(AsrArtifactForSentenceCli),
    Segment(AsrSegmentArtifactForSentenceCli),
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrSegmentArtifactForSentenceCli {
    #[serde(default)]
    task_id: String,
    #[serde(default)]
    media_path: String,
    #[serde(default)]
    source_lang: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    words: Vec<WordTokenCommandDto>,
    segments: Vec<AsrSegmentWithWordsForSentenceCli>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrSegmentWithWordsForSentenceCli {
    #[serde(default)]
    words: Vec<WordTokenCommandDto>,
}

pub fn maybe_run_build_source_sentences_mode_from_args() -> bool {
    const RUN_ARG: &str = "--voxtrans-build-source-sentences";
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 2 || args[1] != RUN_ARG {
        return false;
    }

    let code = match run_build_source_sentences_mode_from_args(&args[2..]) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };
    std::process::exit(code);
}

fn run_build_source_sentences_mode_from_args(args: &[String]) -> Result<(), String> {
    let mut asr_path = String::new();
    let mut output_path = String::new();
    let mut translate_api_key = String::new();
    let mut translate_base_url = String::new();
    let mut translate_model = String::new();
    let mut llm_concurrency = default_llm_concurrency();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--asr-path" => {
                idx += 1;
                asr_path = required_cli_value(args, idx, "--asr-path")?;
            }
            "--output-path" => {
                idx += 1;
                output_path = required_cli_value(args, idx, "--output-path")?;
            }
            "--api-key" => {
                idx += 1;
                translate_api_key = required_cli_value(args, idx, "--api-key")?;
            }
            "--base-url" => {
                idx += 1;
                translate_base_url = required_cli_value(args, idx, "--base-url")?;
            }
            "--model" => {
                idx += 1;
                translate_model = required_cli_value(args, idx, "--model")?;
            }
            "--llm-concurrency" => {
                idx += 1;
                let raw = required_cli_value(args, idx, "--llm-concurrency")?;
                llm_concurrency = raw
                    .parse::<u32>()
                    .map_err(|_| "--llm-concurrency requires integer".to_string())?;
            }
            other => return Err(format!("unknown source-sentences arg: {other}")),
        }
        idx += 1;
    }

    if asr_path.trim().is_empty() {
        return Err("--asr-path is required".to_string());
    }
    if translate_api_key.trim().is_empty()
        || translate_base_url.trim().is_empty()
        || translate_model.trim().is_empty()
    {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if translate_api_key.trim().is_empty() {
            translate_api_key = settings.translate_api_key;
        }
        if translate_base_url.trim().is_empty() {
            translate_base_url = settings.translate_base_url;
        }
        if translate_model.trim().is_empty() {
            translate_model = settings.translate_model;
        }
        if llm_concurrency == default_llm_concurrency() {
            llm_concurrency = settings.llm_concurrency;
        }
    }

    let raw = std::fs::read_to_string(&asr_path).map_err(|err| err.to_string())?;
    let asr = parse_asr_artifact_for_sentence_cli(&raw, &asr_path)?;
    let response = tauri::async_runtime::block_on(build_source_sentences(
        BuildSourceSentencesCommandRequest {
            task_id: asr.task_id.clone(),
            audio_path: asr.media_path.clone(),
            source_lang: asr.source_lang.clone(),
            words: asr.words,
            subtitle_max_words_per_segment: default_subtitle_max_words_per_segment(),
            translate_api_key,
            translate_base_url,
            translate_model,
            llm_concurrency,
        },
    ))?;
    let output_path = if output_path.trim().is_empty() {
        std::path::PathBuf::from(&asr_path)
            .parent()
            .ok_or_else(|| "asr path has no parent directory".to_string())?
            .join("step_02_segments.json")
    } else {
        std::path::PathBuf::from(output_path)
    };
    let segments_payload =
        serde_json::to_string_pretty(&response.segments).map_err(|err| err.to_string())?;
    std::fs::write(&output_path, segments_payload.as_bytes()).map_err(|err| err.to_string())?;

    println!("{}", output_path.display());
    Ok(())
}

fn required_cli_value(args: &[String], idx: usize, flag: &str) -> Result<String, String> {
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{flag} requires value"))
}

fn parse_asr_artifact_for_sentence_cli(
    raw: &str,
    asr_path: &str,
) -> Result<AsrArtifactForSentenceCli, String> {
    let parsed: AsrArtifactForSentenceCliInput =
        serde_json::from_str(raw).map_err(|err| format!("failed to parse asr json: {err}"))?;
    match parsed {
        AsrArtifactForSentenceCliInput::Flat(flat) => Ok(flat),
        AsrArtifactForSentenceCliInput::Segment(segment) => {
            let words = if !segment.words.is_empty() {
                segment.words
            } else {
                segment
                    .segments
                    .into_iter()
                    .flat_map(|entry| entry.words.into_iter())
                    .collect::<Vec<_>>()
            };
            if words.is_empty() {
                return Err("failed to parse asr json: no words found".to_string());
            }

            let task_id = if segment.task_id.trim().is_empty() {
                std::path::Path::new(asr_path)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("task")
                    .to_string()
            } else {
                segment.task_id
            };
            let media_path = if segment.media_path.trim().is_empty() {
                asr_path.to_string()
            } else {
                segment.media_path
            };
            let source_lang = if !segment.source_lang.trim().is_empty() {
                segment.source_lang
            } else if !segment.language.trim().is_empty() {
                segment.language
            } else {
                "auto".to_string()
            };

            Ok(AsrArtifactForSentenceCli {
                task_id,
                media_path,
                source_lang,
                words,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceSentenceCommandDto, WordTokenCommandDto, build_grouped_sentence_segments};

    #[test]
    fn build_grouped_sentence_segments_maps_tokens_by_sentence_span() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.4,
                word: "world".to_string(),
            },
            WordTokenCommandDto {
                start: 0.5,
                end: 0.8,
                word: "Again".to_string(),
            },
        ];
        let sentences = vec![
            SourceSentenceCommandDto {
                sentence_id: 1,
                start_ms: 0,
                end_ms: 400,
                text: "Hello world".to_string(),
                word_start: 0,
                word_end: 1,
                chunk_start: 0,
                chunk_end: 0,
            },
            SourceSentenceCommandDto {
                sentence_id: 2,
                start_ms: 500,
                end_ms: 800,
                text: "Again".to_string(),
                word_start: 2,
                word_end: 2,
                chunk_start: 1,
                chunk_end: 1,
            },
        ];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].segment, "Hello world");
        assert_eq!(segments[0].start, 0.0);
        assert_eq!(segments[0].end, 0.4);
        assert_eq!(segments[0].tokens.len(), 2);
        assert_eq!(segments[0].tokens[0].text, "Hello");
        assert_eq!(segments[0].tokens[1].text, "world");

        assert_eq!(segments[1].segment, "Again");
        assert_eq!(segments[1].start, 0.5);
        assert_eq!(segments[1].end, 0.8);
        assert_eq!(segments[1].tokens.len(), 1);
        assert_eq!(segments[1].tokens[0].text, "Again");
    }

    #[test]
    fn build_grouped_sentence_segments_fallback_preserves_spacing_for_english_and_punctuation() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "Hello".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.3,
                word: ",".to_string(),
            },
            WordTokenCommandDto {
                start: 0.3,
                end: 0.5,
                word: "world".to_string(),
            },
        ];
        let sentences = vec![SourceSentenceCommandDto {
            sentence_id: 1,
            start_ms: 0,
            end_ms: 500,
            text: "   ".to_string(),
            word_start: 0,
            word_end: 2,
            chunk_start: 0,
            chunk_end: 0,
        }];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "Hello, world");
    }

    #[test]
    fn build_grouped_sentence_segments_fallback_preserves_spacing_for_korean() {
        let words = vec![
            WordTokenCommandDto {
                start: 0.0,
                end: 0.2,
                word: "안녕하세요".to_string(),
            },
            WordTokenCommandDto {
                start: 0.2,
                end: 0.4,
                word: "여러분".to_string(),
            },
        ];
        let sentences = vec![SourceSentenceCommandDto {
            sentence_id: 1,
            start_ms: 0,
            end_ms: 400,
            text: "".to_string(),
            word_start: 0,
            word_end: 1,
            chunk_start: 0,
            chunk_end: 0,
        }];

        let segments = build_grouped_sentence_segments(&words, &sentences);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "안녕하세요 여러분");
    }
}
use std::sync::Arc;
