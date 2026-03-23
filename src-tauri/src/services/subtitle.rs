use serde::Deserialize;
use sqlx::SqlitePool;
use voxtrans_core::subtitle::srt::{normalize_cues, parse_srt, to_srt_from_cues};

use crate::services::final_subtitle::{
    FinalSubtitleSegment, cues_to_final_subtitle_segments, normalize_final_subtitle_segments_json,
    parse_final_subtitle_segments,
};

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
        .and_then(normalize_final_subtitle_segments_json)
        .map(Ok)
        .unwrap_or_else(|| build_final_subtitle_segments_json(&normalized, &row.subtitle_segments_json))?;

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

fn build_final_subtitle_segments_json(
    cues: &[voxtrans_core::subtitle::srt::SrtCue],
    existing_segments_raw: &str,
) -> Result<String, String> {
    let existing_segments = parse_final_subtitle_segments(existing_segments_raw);
    let merged = cues_to_final_subtitle_segments(cues, &existing_segments)
        .into_iter()
        .map(|segment| FinalSubtitleSegment {
            translated_text: best_translated_text(
                segment.start_ms,
                segment.end_ms,
                &segment.translated_text,
                &existing_segments,
            ),
            ..segment
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&merged).map_err(|err| err.to_string())
}

fn best_translated_text(
    start_ms: i64,
    end_ms: i64,
    fallback: &str,
    existing: &[FinalSubtitleSegment],
) -> String {
    let mut best_overlap = -1_i64;
    let mut best_text = fallback.to_string();
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
