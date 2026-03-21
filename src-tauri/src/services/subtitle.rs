use serde::Deserialize;
use sqlx::SqlitePool;
use voxtrans_core::subtitle::srt::{normalize_cues, parse_srt, to_srt_from_cues};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSaveRequest {
    pub task_id: String,
    pub content: String,
    #[serde(default)]
    pub subtitle_segments_json: Option<String>,
}

pub async fn save_subtitle_editor(
    pool: &SqlitePool,
    request: SubtitleSaveRequest,
) -> Result<(), String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    let parsed = parse_srt(&request.content).map_err(|err| err.to_string())?;
    let normalized = normalize_cues(&parsed);
    let normalized_srt = to_srt_from_cues(&normalized);
    let row = sqlx::query_as::<_, TaskRunEditorRow>(
        "SELECT subtitle_segments_json, translated_srt
         FROM task_runs WHERE id = ?",
    )
    .bind(request.task_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "task not found".to_string())?;

    let merged_segments_json = request
        .subtitle_segments_json
        .as_deref()
        .and_then(normalize_subtitle_segments_json)
        .unwrap_or(build_segments_json_with_preserved_translation(
            &normalized,
            &row.subtitle_segments_json,
        )?);

    let now = unix_now();
    sqlx::query(
        "UPDATE task_runs
         SET result_srt = ?, subtitle_segments_json = ?, translated_srt = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(normalized_srt)
    .bind(merged_segments_json)
    .bind(row.translated_srt)
    .bind(now)
    .bind(request.task_id.trim())
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct TaskRunEditorRow {
    subtitle_segments_json: String,
    translated_srt: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleSegmentValue {
    start_ms: i64,
    end_ms: i64,
    #[serde(default)]
    translated_text: String,
}

fn build_segments_json_with_preserved_translation(
    cues: &[voxtrans_core::subtitle::srt::SrtCue],
    existing_segments_raw: &str,
) -> Result<String, String> {
    let existing_segments = serde_json::from_str::<Vec<SubtitleSegmentValue>>(existing_segments_raw)
        .unwrap_or_default();
    let merged = cues
        .iter()
        .map(|cue| {
            let translated = find_best_translated_text(cue.start_ms as i64, cue.end_ms as i64, &existing_segments);
            serde_json::json!({
                "startMs": cue.start_ms as i64,
                "endMs": cue.end_ms as i64,
                "sourceText": cue.text.clone(),
                "translatedText": translated,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&merged).map_err(|err| err.to_string())
}

fn normalize_subtitle_segments_json(raw: &str) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let arr = parsed.as_array()?;
    let normalized = arr
        .iter()
        .map(|segment| {
            let start = segment
                .get("startMs")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .round() as i64;
            let end = segment
                .get("endMs")
                .and_then(|v| v.as_f64())
                .unwrap_or(start as f64)
                .round() as i64;
            let source = segment
                .get("sourceText")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let translated = segment
                .get("translatedText")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            serde_json::json!({
                "startMs": start.max(0),
                "endMs": end.max(start),
                "sourceText": source,
                "translatedText": translated,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&normalized).ok()
}

fn find_best_translated_text(
    start_ms: i64,
    end_ms: i64,
    existing: &[SubtitleSegmentValue],
) -> String {
    let mut best_overlap = -1_i64;
    let mut best_text = String::new();
    for segment in existing {
        let overlap = overlap_ms(start_ms, end_ms, segment.start_ms, segment.end_ms);
        if overlap > best_overlap {
            best_overlap = overlap;
            best_text = segment.translated_text.clone();
        }
    }
    best_text
}

fn overlap_ms(a_start: i64, a_end: i64, b_start: i64, b_end: i64) -> i64 {
    let left = a_start.max(b_start);
    let right = a_end.min(b_end);
    (right - left).max(0)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
