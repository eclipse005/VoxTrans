//! Dump step_01_asr artifacts from the GUI SQLite DB into JSON fixtures
//! consumable by `step2-repl`.
//!
//! Usage:
//!     export-asr-artifacts --db <voxtrans.db> [--out <dir>]
//!
//! Produces one `task_<index>_asr.json` per task, containing only the fields
//! step2-repl needs (words, vadSpeechSegments, sourceLang, subtitleLengthPreset).

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Minimal subset of the step_01_asr artifact that step2-repl consumes.
/// We extract these fields and drop everything else (taskId, mediaPath, text,
/// alignedText, timing, ...) to keep fixtures small and focused.
#[derive(Serialize)]
struct Fixture {
    words: Value,
    #[serde(rename = "vadSpeechSegments")]
    vad_speech_segments: Value,
    #[serde(rename = "sourceLang")]
    source_lang: String,
    #[serde(rename = "subtitleLengthPreset")]
    subtitle_length_preset: String,
}

struct Args {
    db: PathBuf,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = env::args().skip(1);
    let mut db: Option<PathBuf> = None;
    let mut out = PathBuf::from("fixtures");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(PathBuf::from(args.next().ok_or("--db requires a value")?)),
            "--out" => out = PathBuf::from(args.next().ok_or("--out requires a value")?),
            "--help" | "-h" => {
                return Err(
                    "usage: export-asr-artifacts --db <voxtrans.db> [--out <dir>]"
                        .to_string(),
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let db = db.ok_or("missing --db <path>\nusage: export-asr-artifacts --db <voxtrans.db> [--out <dir>]")?;
    Ok(Args { db, out })
}

/// Run a sqlite3 query to fetch artifact payloads, avoiding a heavy sqlx
/// async runtime setup in this tiny CLI. Falls back with a clear error if
/// sqlite3 is not on PATH.
fn fetch_artifacts(db_path: &PathBuf) -> Result<Vec<(String, String)>, String> {
    let output = std::process::Command::new("sqlite3")
        .arg(db_path)
        .arg("-json")
        .arg("SELECT task_id, payload_json FROM task_artifacts WHERE step_name = 'step_01_asr' ORDER BY task_id;")
        .output()
        .map_err(|e| format!("failed to run sqlite3 (is it on PATH?): {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "sqlite3 failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<Record> =
        serde_json::from_str(&stdout).map_err(|e| format!("failed to parse sqlite3 JSON: {e}"))?;
    Ok(rows.into_iter().map(|r| (r.task_id, r.payload_json)).collect())
}

#[derive(Deserialize)]
struct Record {
    task_id: String,
    payload_json: String,
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    fs::create_dir_all(&args.out).unwrap_or_else(|e| {
        eprintln!("warning: could not create {}: {e}", args.out.display());
    });

    let artifacts = match fetch_artifacts(&args.db) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    if artifacts.is_empty() {
        eprintln!("no step_01_asr artifacts found in {}", args.db.display());
        std::process::exit(0);
    }

    eprintln!("found {} artifacts, exporting to {}/", artifacts.len(), args.out.display());

    for (index, (task_id, payload)) in artifacts.iter().enumerate() {
        let artifact: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  [{index}] {task_id}: skip (bad JSON: {e})");
                continue;
            }
        };

        // The ASR artifact stores words and vadSpeechSegments; sourceLang is
        // present too. subtitleLengthPreset is NOT in the artifact — use the
        // default "standard" (the user can override via step2-repl --preset).
        let fixture = Fixture {
            words: artifact.get("words").cloned().unwrap_or(Value::Array(vec![])),
            vad_speech_segments: artifact
                .get("vadSpeechSegments")
                .cloned()
                .unwrap_or(Value::Array(vec![])),
            source_lang: artifact
                .get("sourceLang")
                .and_then(|v| v.as_str())
                .unwrap_or("en")
                .to_string(),
            subtitle_length_preset: "standard".to_string(),
        };

        let filename = format!("task_{}.json", index + 1);
        let out_path = args.out.join(&filename);
        let json = serde_json::to_string_pretty(&fixture).unwrap_or_default();
        match fs::write(&out_path, json) {
            Ok(()) => {
                let word_count = fixture.words.as_array().map(|a| a.len()).unwrap_or(0);
                let vad_count = fixture.vad_speech_segments.as_array().map(|a| a.len()).unwrap_or(0);
                eprintln!(
                    "  [{index}] {task_id} -> {filename}  (words={word_count}, vad={vad_count}, lang={})",
                    fixture.source_lang
                );
            }
            Err(e) => eprintln!("  [{index}] {task_id}: write failed: {e}"),
        }
    }
}
