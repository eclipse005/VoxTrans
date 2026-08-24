//! Replay the terminology agent against an existing SRT transcript.
//! Not invoked by the app; used to A/B agent changes (rounds, tokens,
//! glossary quality) without re-running ASR.
//!
//! Settings JSON (camelCase):
//!   taskId, sourceLang, targetLang, translateApiKey, translateBaseUrl,
//!   translateModel, srtPath, mediaPath (for log placement), title
//!
//! After the run it prints the agent log path; sum usage from agent.round
//! events there. Pass --candidates-only to skip the LLM and just dump the
//! deterministic candidate list.

use std::fs;
use std::path::PathBuf;

use voxtrans::services::terminology_agent::{
    merge_glossary_user_priority, run_terminology_briefing, GlossaryEntry, TerminologyAgentInput,
    TranscriptCue,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayUserTerm {
    source: String,
    target: String,
    #[serde(default)]
    note: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySettings {
    task_id: String,
    source_lang: String,
    target_lang: String,
    translate_api_key: String,
    translate_base_url: String,
    translate_model: String,
    srt_path: String,
    media_path: String,
    title: String,
    #[serde(default)]
    user_terms: Vec<ReplayUserTerm>,
    #[serde(default)]
    anysearch_api_key: String,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("replay failed: {err}");
        std::process::exit(1);
    }
}

fn parse_srt(path: &str) -> Result<Vec<TranscriptCue>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read srt: {e}"))?;
    let mut cues = Vec::new();
    for block in content.split("\n\n") {
        let lines: Vec<&str> = block.trim().lines().collect();
        if lines.len() < 3 {
            continue;
        }
        let index: usize = lines[0].trim().parse().unwrap_or(cues.len() + 1);
        let start_ms = parse_start_ms(lines[1]).unwrap_or(0);
        cues.push(TranscriptCue {
            index,
            start_ms,
            text: lines[2..].join(" "),
        });
    }
    Ok(cues)
}

fn parse_start_ms(timing: &str) -> Option<u64> {
    let start = timing.split("-->").next()?.trim();
    let (hms, ms) = start.split_once(&[',', '.'][..])?;
    let mut it = hms.split(':');
    let h: u64 = it.next()?.parse().ok()?;
    let m: u64 = it.next()?.parse().ok()?;
    let s: u64 = it.next()?.parse().ok()?;
    let ms: u64 = ms.trim().parse().ok()?;
    Some(((h * 3600 + m * 60 + s) * 1000) + ms)
}

async fn run() -> Result<(), String> {
    let settings_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"C:\Users\ADMIN\AppData\Local\Temp\vt-replay-terminology.json".into());
    let settings: ReplaySettings = serde_json::from_str(
        &fs::read_to_string(&settings_path).map_err(|e| format!("read settings: {e}"))?,
    )
    .map_err(|e| format!("parse settings: {e}"))?;

    let cues = parse_srt(&settings.srt_path)?;
    let total_chars: usize = cues.iter().map(|c| c.text.chars().count()).sum();
    eprintln!(
        "loaded {} cues ({} chars) from {}",
        cues.len(),
        total_chars,
        settings.srt_path
    );

    if std::env::args().any(|a| a == "--candidates-only") {
        let media = PathBuf::from(&settings.media_path);
        let out = media
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("candidates_dump.txt");
        let block = voxtrans::services::terminology_agent::debug_candidates_block(&cues);
        fs::write(&out, &block).map_err(|e| e.to_string())?;
        eprintln!("wrote {}", out.display());
        return Ok(());
    }

    let user_terms: Vec<GlossaryEntry> = settings
        .user_terms
        .iter()
        .map(|t| GlossaryEntry::new(&t.source, &t.target, &t.note))
        .collect();
    eprintln!("user terms injected: {}", user_terms.len());

    let briefing = run_terminology_briefing(
        TerminologyAgentInput {
            task_id: settings.task_id.clone(),
            media_path: settings.media_path.clone(),
            title: settings.title.clone(),
            source_lang: settings.source_lang.clone(),
            target_lang: settings.target_lang.clone(),
            cues,
            user_terms: user_terms.clone(),
            api_key: settings.translate_api_key.clone(),
            base_url: settings.translate_base_url.clone(),
            model: settings.translate_model.clone(),
            anysearch_api_key: settings.anysearch_api_key.clone(),
            store: None,
        },
        |win, nwin, round, max, tool| {
            eprintln!(
                "window {win}/{nwin} round {round}/{max} {}",
                tool.unwrap_or_default()
            );
        },
    )
    .await;

    eprintln!(
        "briefing: skipped={} end={} windows={} glossary={} style_chars={}",
        briefing.skipped,
        briefing.end_reason.as_str(),
        briefing.windows,
        briefing.glossary.len(),
        briefing.style_guide.chars().count()
    );
    // Mirror the app pipeline (translation_flow.rs): user terms always kept
    // and win over agent rows on the same normalized source key.
    let hay: String = cues_haystack(&settings.srt_path)?;
    let merged = merge_glossary_user_priority(&user_terms, &briefing.glossary, &hay);
    let dump = PathBuf::from(&settings.media_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("briefing_replay.json");
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "skipped": briefing.skipped,
        "endReason": briefing.end_reason.as_str(),
        "skipReason": briefing.skip_reason,
        "windows": briefing.windows,
        "glossary": briefing.glossary,
        "mergedGlossary": merged,
        "styleGuide": briefing.style_guide,
    }))
    .map_err(|e| e.to_string())?;
    fs::write(&dump, json).map_err(|e| e.to_string())?;
    eprintln!("wrote {}", dump.display());
    Ok(())
}

fn cues_haystack(srt_path: &str) -> Result<String, String> {
    Ok(parse_srt(srt_path)?
        .iter()
        .map(|c| c.text.clone())
        .collect::<Vec<_>>()
        .join("\n"))
}
