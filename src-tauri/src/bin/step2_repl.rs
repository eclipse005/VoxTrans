//! Offline Step2 segmentation REPL.
//!
//! Reads a JSON fixture (words + VAD segments + lang + preset) produced by
//! `export-asr-artifacts`, runs the sentence-boundary pipeline, and prints a
//! statistics report. Optionally writes the resulting SRT.
//!
//! Usage:
//!     step2-repl <input.json> [--preset standard|short|loose] [--out output.srt]
//!
//! The JSON schema (also accepts the raw step_01_asr artifact, which has
//! extra fields like `taskId`/`mediaPath`/`text` that are ignored):
//! ```json
//! {
//!   "words": [{"start": 0.56, "end": 0.72, "word": "All"}, ...],
//!   "vadSpeechSegments": [[0.0, 2.3], [2.8, 5.1], ...],
//!   "sourceLang": "en",
//!   "subtitleLengthPreset": "standard"
//! }
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use serde::Deserialize;
use voxtrans::services::transcribe::WordTokenDto;
use voxtrans::services::transcription::{
    SentenceBoundaryRequest, build_source_sentences_from_words_with_progress, source_sentences_to_srt,
};

/// Input fixture. Field names use snake_case in Rust but serde aliases accept
/// the camelCase form stored in the ASR artifact JSON.
#[derive(Deserialize)]
#[allow(dead_code)]
struct ReplInput {
    words: Vec<WordTokenDto>,
    #[serde(default, alias = "vadSpeechSegments")]
    vad_speech_segments: Vec<(f64, f64)>,
    #[serde(default = "default_lang", alias = "sourceLang")]
    source_lang: String,
    #[serde(default = "default_preset", alias = "subtitleLengthPreset")]
    subtitle_length_preset: String,
}

fn default_lang() -> String {
    "en".to_string()
}

fn default_preset() -> String {
    "standard".to_string()
}

/// True for CJK languages that use character-based (not word-based) counting.
fn is_cjk_lang(lang: &str) -> bool {
    let l = lang.trim().to_ascii_lowercase();
    l.starts_with("zh") || l.starts_with("yue") || l.starts_with("ja") || l.starts_with("ko")
}

struct Args {
    input: PathBuf,
    preset: Option<String>,
    out: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);
    let input = args
        .next()
        .ok_or_else(|| "missing input JSON path\nusage: step2-repl <input.json> [--preset <p>] [--out <srt>]".to_string())?;
    if input == "--help" || input == "-h" {
        return Err("usage: step2-repl <input.json> [--preset standard|short|loose] [--out output.srt]".to_string());
    }

    let mut preset = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--preset" => {
                preset = Some(args.next().ok_or("--preset requires a value")?);
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or("--out requires a value")?,
                ));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        input: PathBuf::from(input),
        preset,
        out,
    })
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let json = match fs::read_to_string(&args.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {e}", args.input.display());
            std::process::exit(1);
        }
    };

    let input: ReplInput = match serde_json::from_str(&json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("failed to parse JSON: {e}");
            std::process::exit(1);
        }
    };

    let preset = args.preset.unwrap_or(input.subtitle_length_preset.clone());
    let vad_count = input.vad_speech_segments.len();
    let word_count = input.words.len();

    let request = SentenceBoundaryRequest {
        task_id: "repl".to_string(),
        media_path: String::new(),
        source_lang: input.source_lang.clone(),
        subtitle_length_preset: preset.clone(),
        use_subtitle_layout_split: true,
        words: input.words,
        vad_speech_segments: input.vad_speech_segments,
    };

    let t0 = Instant::now();
    let step2 =
        tauri::async_runtime::block_on(build_source_sentences_from_words_with_progress(
            request, None,
        ));
    let elapsed = t0.elapsed();

    let step2 = match step2 {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Step2 failed: {e}");
            std::process::exit(1);
        }
    };

    // --- statistics ---
    // Language-aware unit counting: CJK languages have no word boundaries, so
    // count characters instead of whitespace-split words.
    let is_cjk = is_cjk_lang(&input.source_lang);
    let sentences = &step2.translation_sentences;
    let total = sentences.len();
    let word_counts: Vec<usize> = sentences
        .iter()
        .map(|s| {
            if is_cjk {
                s.text.chars().filter(|c| !c.is_whitespace()).count()
            } else {
                s.text.split_whitespace().count()
            }
        })
        .collect();
    let unit_label = if is_cjk { "chars" } else { "words" };
    // For CJK the "short" threshold is higher (chars are finer-grained).
    let short_threshold = if is_cjk { 8 } else { 4 };
    let short = word_counts.iter().filter(|&&w| w <= short_threshold).count();
    let max_words = word_counts.iter().copied().max().unwrap_or(0);
    let avg_words = if total > 0 {
        word_counts.iter().sum::<usize>() as f64 / total as f64
    } else {
        0.0
    };

    let mut terminal = 0usize;
    let mut layout = 0usize;
    let mut merge = 0usize;
    for b in &step2.boundaries {
        match b.reason_tag.as_str() {
            "terminal_punctuation" => terminal += 1,
            "subtitle_layout" => layout += 1,
            "merge" => merge += 1,
            _ => {}
        }
    }

    let vad_count_actual = vad_count;

    println!("── Step2 segmentation report ──");
    println!("language:        {}", input.source_lang);
    println!("preset:          {preset}");
    println!("words (input):   {word_count}");
    println!("vad segments:    {vad_count_actual}");
    println!("sentences:       {total}");
    println!("short (<=4 w):   {short} ({pct}%)", pct = if total > 0 { short * 100 / total } else { 0 });
    println!("avg {unit_label}/sent:  {avg_words:.1}");
    println!("max {unit_label}/sent:  {max_words}");
    println!("splits:          terminal_punctuation={terminal}, subtitle_layout={layout}, merge={merge}");
    println!("elapsed:         {:.2}ms", elapsed.as_secs_f64() * 1000.0);

    // --- write SRT ---
    let out_path = args.out.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("srt");
        p
    });
    let srt = source_sentences_to_srt(&step2);
    match fs::write(&out_path, srt) {
        Ok(()) => println!("srt written:      {}", out_path.display()),
        Err(e) => eprintln!("failed to write SRT: {e}"),
    }
}
