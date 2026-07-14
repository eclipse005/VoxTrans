use std::fs;
use std::path::{Path, PathBuf};

use crate::services::subtitle_srt::is_reserved_task_srt_basename;
use crate::services::task_path::{sanitize_filename_component, task_output_dir};
use crate::services::workspace_subtitle::{
    WorkspaceSubtitleSegment, serialize_segments,
};

/// Materialize an uploaded SRT into the task output directory and parse cues.
///
/// - Creates `output/{stem}_{taskId}/{originalName}.srt` when safe
/// - If the basename collides with completion outputs (`trans.srt`, etc.),
///   renames the on-disk copy to `{stem}_original.srt` (keeps UI name separate)
/// - Returns the absolute path of the copy, file size, segments JSON, and source text
pub fn import_srt_for_task(
    task_id: &str,
    source_path: &Path,
    display_name: &str,
) -> Result<SrtImportResult, String> {
    if !source_path.is_file() {
        return Err(format!(
            "SRT file not found: {}",
            source_path.display()
        ));
    }

    let raw = fs::read(source_path).map_err(|err| {
        format!(
            "failed to read SRT {}: {err}",
            source_path.display()
        )
    })?;
    let content = decode_srt_bytes(&raw)?;

    let cues = voxtrans_core::subtitle::srt::parse_srt_content(&content)?;
    let segments: Vec<WorkspaceSubtitleSegment> = cues
        .iter()
        .map(|cue| WorkspaceSubtitleSegment {
            start_ms: cue.start_ms,
            end_ms: cue.end_ms,
            source_text: cue.text.clone(),
            translated_text: String::new(),
            source_words: Vec::new(),
        })
        .collect();

    let preferred_file_name = preferred_srt_file_name(display_name, source_path);
    let disk_file_name = resolve_non_colliding_import_basename(&preferred_file_name);
    let renamed_for_reserved = !disk_file_name.eq_ignore_ascii_case(&preferred_file_name);

    let stem = Path::new(&disk_file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Path::new(&preferred_file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "subtitle".to_string());

    // task_output_dir uses media path stem; pass a synthetic path so the
    // directory is named {stem}_{taskId} under the global output root.
    // Prefer the *display* stem so folder names stay user-recognizable even
    // when the on-disk file was renamed to avoid reserved export basenames.
    let folder_stem = Path::new(&preferred_file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stem.clone());
    let synthetic = PathBuf::from(format!("{folder_stem}.srt"));
    let task_dir = task_output_dir(task_id, &synthetic);
    fs::create_dir_all(&task_dir).map_err(|err| {
        format!(
            "failed to create task directory {}: {err}",
            task_dir.display()
        )
    })?;

    let dest = task_dir.join(&disk_file_name);
    fs::write(&dest, content.as_bytes()).map_err(|err| {
        format!("failed to copy SRT to {}: {err}", dest.display())
    })?;

    let size_bytes = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    let source_text = segments
        .iter()
        .map(|s| s.source_text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let result_srt = voxtrans_core::subtitle::srt::to_srt_from_cues(&cues);

    Ok(SrtImportResult {
        path: dest.to_string_lossy().to_string(),
        size_bytes,
        subtitle_segments_json: serialize_segments(&segments),
        result_text: source_text,
        result_srt,
        cue_count: segments.len(),
        safe_stem: sanitize_filename_component(&folder_stem),
        disk_file_name,
        renamed_for_reserved,
    })
}

pub struct SrtImportResult {
    pub path: String,
    pub size_bytes: u64,
    pub subtitle_segments_json: String,
    pub result_text: String,
    pub result_srt: String,
    pub cue_count: usize,
    pub safe_stem: String,
    /// On-disk basename inside the task folder (may differ from upload name).
    pub disk_file_name: String,
    /// True when the file was renamed to avoid clobbering completion outputs.
    pub renamed_for_reserved: bool,
}

pub fn is_srt_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("srt"))
        .unwrap_or(false)
}

/// Decode SRT file bytes: UTF-8 (with optional BOM). Clear error otherwise.
fn decode_srt_bytes(raw: &[u8]) -> Result<String, String> {
    let bytes = raw
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .unwrap_or(raw);
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        "SRT must be UTF-8 encoded (optional BOM allowed). Convert the file encoding and try again."
            .to_string()
    })
}

fn preferred_srt_file_name(display_name: &str, source_path: &Path) -> String {
    let original = Path::new(display_name)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            source_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "subtitle.srt".to_string());
    ensure_srt_extension(sanitize_filename_component(&original))
}

/// Ensure the imported basename never collides with fixed completion outputs.
///
/// Strategy (best practice — rename, do not reject):
/// 1. Prefer the sanitized original name when free
/// 2. Else `{stem}_original.srt`
/// 3. Else `{stem}_original_{n}.srt`
pub fn resolve_non_colliding_import_basename(preferred: &str) -> String {
    let preferred = ensure_srt_extension(sanitize_filename_component(preferred));
    if !is_reserved_task_srt_basename(&preferred) {
        return preferred;
    }

    let stem = Path::new(&preferred)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "subtitle".to_string());
    // Avoid stacking: if user uploaded "trans.srt", stem is "trans" → "trans_original.srt"
    let base = format!("{stem}_original.srt");
    let base = ensure_srt_extension(sanitize_filename_component(&base));
    if !is_reserved_task_srt_basename(&base) {
        return base;
    }

    for n in 1..=99 {
        let candidate = ensure_srt_extension(sanitize_filename_component(&format!(
            "{stem}_original_{n}.srt"
        )));
        if !is_reserved_task_srt_basename(&candidate) {
            return candidate;
        }
    }
    // Last resort — still should not hit reserved list.
    ensure_srt_extension(sanitize_filename_component(&format!(
        "imported_{stem}.srt"
    )))
}

fn ensure_srt_extension(name: String) -> String {
    if name.is_empty() {
        return "subtitle.srt".to_string();
    }
    if name.to_ascii_lowercase().ends_with(".srt") {
        name
    } else {
        format!("{name}.srt")
    }
}

/// Load segments for a subtitle task: prefer in-memory JSON, else re-parse the copied SRT.
pub fn load_srt_segments_for_run(
    subtitle_segments_json: &str,
    media_path: &str,
) -> Result<Vec<WorkspaceSubtitleSegment>, String> {
    if !subtitle_segments_json.trim().is_empty() && subtitle_segments_json.trim() != "[]" {
        let segments: Vec<WorkspaceSubtitleSegment> =
            serde_json::from_str(subtitle_segments_json).map_err(|err| {
                format!("invalid subtitleSegmentsJson: {err}")
            })?;
        if !segments.is_empty() {
            return Ok(segments);
        }
    }

    let raw = fs::read(media_path).map_err(|err| {
        format!("failed to read SRT for run {media_path}: {err}")
    })?;
    let content = decode_srt_bytes(&raw)?;
    let cues = voxtrans_core::subtitle::srt::parse_srt_content(&content)?;
    Ok(cues
        .into_iter()
        .map(|cue| WorkspaceSubtitleSegment {
            start_ms: cue.start_ms,
            end_ms: cue.end_ms,
            source_text: cue.text,
            translated_text: String::new(),
            source_words: Vec::new(),
        })
        .collect())
}

pub fn workspace_segments_to_step2(
    segments: &[WorkspaceSubtitleSegment],
) -> Vec<crate::commands::transcription::GroupedSentenceSegmentCommandDto> {
    segments
        .iter()
        .map(|segment| crate::commands::transcription::GroupedSentenceSegmentCommandDto {
            segment: segment.source_text.clone(),
            start: segment.start_ms as f64 / 1000.0,
            end: segment.end_ms as f64 / 1000.0,
            tokens: Vec::new(),
        })
        .collect()
}

pub fn source_text_from_workspace_segments(segments: &[WorkspaceSubtitleSegment]) -> String {
    segments
        .iter()
        .map(|s| s.source_text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_normal_names() {
        assert_eq!(
            resolve_non_colliding_import_basename("episode01.srt"),
            "episode01.srt"
        );
    }

    #[test]
    fn resolve_renames_reserved_export_names() {
        assert_eq!(
            resolve_non_colliding_import_basename("trans.srt"),
            "trans_original.srt"
        );
        assert_eq!(
            resolve_non_colliding_import_basename("src_trans.srt"),
            "src_trans_original.srt"
        );
        assert_eq!(
            resolve_non_colliding_import_basename("SRC.SRT"),
            "SRC_original.srt"
        );
    }

    #[test]
    fn decode_strips_utf8_bom() {
        let mut raw = vec![0xEF, 0xBB, 0xBF];
        raw.extend_from_slice(b"1\n00:00:00,000 --> 00:00:01,000\nHi\n");
        let text = decode_srt_bytes(&raw).expect("utf8");
        assert!(text.starts_with('1'));
    }
}
