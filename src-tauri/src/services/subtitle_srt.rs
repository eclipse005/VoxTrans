use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSrtSegment {
    #[serde(default)]
    pub start_ms: u64,
    #[serde(default)]
    pub end_ms: u64,
    #[serde(default)]
    pub source_text: String,
    #[serde(default)]
    pub translated_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportSrtItem {
    Source,
    Target,
    BilingualSourceFirst,
    BilingualTargetFirst,
}

#[derive(Debug, Clone)]
pub struct SubtitleBeautifyOptions {
    pub enabled: bool,
    pub subtitle_length_preset: String,
    pub target_lang: String,
}

impl ExportSrtItem {
    pub fn output_file_name(self) -> &'static str {
        match self {
            ExportSrtItem::Source => "src.srt",
            ExportSrtItem::Target => "trans.srt",
            ExportSrtItem::BilingualSourceFirst => "src_trans.srt",
            ExportSrtItem::BilingualTargetFirst => "trans_src.srt",
        }
    }

    pub fn requires_translation(self) -> bool {
        !matches!(self, ExportSrtItem::Source)
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "source" => Some(ExportSrtItem::Source),
            "target" => Some(ExportSrtItem::Target),
            "bilingualSourceFirst" => Some(ExportSrtItem::BilingualSourceFirst),
            "bilingualTargetFirst" => Some(ExportSrtItem::BilingualTargetFirst),
            _ => None,
        }
    }
}

pub fn write_task_output_variants_for_completion(
    task_id: &str,
    media_path: &Path,
    mut segments: Vec<SubtitleSrtSegment>,
    include_translation_variants: bool,
    beautify: SubtitleBeautifyOptions,
) -> Result<Vec<String>, String> {
    if beautify.enabled {
        crate::services::subtitle_beautify::beautify_subtitle_srt_segments(
            &mut segments,
            &beautify.subtitle_length_preset,
            &beautify.target_lang,
        );
    }
    let items = if include_translation_variants {
        vec![
            ExportSrtItem::Source,
            ExportSrtItem::Target,
            ExportSrtItem::BilingualSourceFirst,
            ExportSrtItem::BilingualTargetFirst,
        ]
    } else {
        vec![ExportSrtItem::Source]
    };
    write_task_output_variants(task_id, media_path, &segments, &items)
}

pub fn parse_segments_json(raw: &str) -> Result<Vec<SubtitleSrtSegment>, String> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<SubtitleSrtSegment>>(raw).map_err(|err| err.to_string())
}

pub fn has_translated_content(segments: &[SubtitleSrtSegment]) -> bool {
    segments
        .iter()
        .any(|segment| !normalize_inline_text(&segment.translated_text).is_empty())
}

pub fn write_variants_to_directory(
    target_dir: &Path,
    segments: &[SubtitleSrtSegment],
    items: &[ExportSrtItem],
) -> Result<Vec<String>, String> {
    if items.is_empty() {
        return Err("items is required".to_string());
    }
    if !target_dir.is_dir() {
        return Err(format!("导出目录不存在: {}", target_dir.display()));
    }
    validate_translation_requirement(segments, items)?;

    let mut seen = HashSet::<ExportSrtItem>::new();
    let mut written = Vec::<String>::new();
    for item in items {
        if !seen.insert(*item) {
            continue;
        }
        let output_path = target_dir.join(item.output_file_name());
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let content = build_variant_srt(segments, *item);
        std::fs::write(&output_path, content.as_bytes()).map_err(|err| err.to_string())?;
        written.push(output_path.display().to_string());
    }
    Ok(written)
}

pub fn write_flat_variants_to_directory(
    target_dir: &Path,
    file_stem: &str,
    segments: &[SubtitleSrtSegment],
    items: &[ExportSrtItem],
) -> Result<Vec<String>, String> {
    if items.is_empty() {
        return Err("items is required".to_string());
    }
    if !target_dir.is_dir() {
        return Err(format!("导出目录不存在: {}", target_dir.display()));
    }
    validate_translation_requirement(segments, items)?;

    let mut seen = HashSet::<ExportSrtItem>::new();
    let mut written = Vec::<String>::new();
    for item in items {
        if !seen.insert(*item) {
            continue;
        }
        let suffix = match item {
            ExportSrtItem::Source => "src",
            ExportSrtItem::Target => "trans",
            ExportSrtItem::BilingualSourceFirst => "src_trans",
            ExportSrtItem::BilingualTargetFirst => "trans_src",
        };
        let output_path = target_dir.join(format!("{file_stem}_{suffix}.srt"));
        let content = build_variant_srt(segments, *item);
        std::fs::write(&output_path, content.as_bytes()).map_err(|err| err.to_string())?;
        written.push(output_path.display().to_string());
    }
    Ok(written)
}

pub fn write_task_output_variants(
    task_id: &str,
    media_path: &Path,
    segments: &[SubtitleSrtSegment],
    items: &[ExportSrtItem],
) -> Result<Vec<String>, String> {
    if task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if items.is_empty() {
        return Err("items is required".to_string());
    }
    validate_translation_requirement(segments, items)?;

    let mut seen = HashSet::<ExportSrtItem>::new();
    let mut written = Vec::<String>::new();
    for item in items {
        if !seen.insert(*item) {
            continue;
        }
        let output_path = task_output_path(task_id, media_path, *item);
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let content = build_variant_srt(segments, *item);
        std::fs::write(&output_path, content.as_bytes()).map_err(|err| err.to_string())?;
        written.push(output_path.display().to_string());
    }
    Ok(written)
}

pub fn task_output_path(task_id: &str, media_path: &Path, item: ExportSrtItem) -> PathBuf {
    match item {
        ExportSrtItem::Source => {
            crate::services::task_path::task_src_srt_output_path(task_id, media_path)
        }
        ExportSrtItem::Target => {
            crate::services::task_path::task_trans_srt_output_path(task_id, media_path)
        }
        ExportSrtItem::BilingualSourceFirst => {
            crate::services::task_path::task_src_trans_srt_output_path(task_id, media_path)
        }
        ExportSrtItem::BilingualTargetFirst => {
            crate::services::task_path::task_trans_src_srt_output_path(task_id, media_path)
        }
    }
}

pub fn build_variant_srt(segments: &[SubtitleSrtSegment], item: ExportSrtItem) -> String {
    let mut out = String::new();
    let mut next_index = 1usize;
    for segment in segments {
        let source_text = normalize_inline_text(&segment.source_text);
        let translated_text = normalize_inline_text(&segment.translated_text);
        let body = match item {
            ExportSrtItem::Source => source_text,
            ExportSrtItem::Target => translated_text,
            ExportSrtItem::BilingualSourceFirst => join_bilingual(&source_text, &translated_text),
            ExportSrtItem::BilingualTargetFirst => join_bilingual(&translated_text, &source_text),
        };
        if body.is_empty() {
            continue;
        }
        let start_ms = segment.start_ms;
        let end_ms = segment.end_ms.max(start_ms);
        out.push_str(&next_index.to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_srt_ms(start_ms),
            format_srt_ms(end_ms)
        ));
        out.push_str(&body);
        out.push_str("\n\n");
        next_index += 1;
    }
    out.trim_end().to_string()
}

fn validate_translation_requirement(
    segments: &[SubtitleSrtSegment],
    items: &[ExportSrtItem],
) -> Result<(), String> {
    if !items.iter().any(|item| item.requires_translation()) {
        return Ok(());
    }
    if has_translated_content(segments) {
        return Ok(());
    }
    Err("当前任务暂无译文，无法导出译文相关字幕".to_string())
}

fn join_bilingual(top: &str, bottom: &str) -> String {
    match (top.is_empty(), bottom.is_empty()) {
        (true, true) => String::new(),
        (false, true) => top.to_string(),
        (true, false) => bottom.to_string(),
        (false, false) => format!("{top}\n{bottom}"),
    }
}

fn normalize_inline_text(raw: &str) -> String {
    raw.trim()
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn format_srt_ms(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_variant_srt_uses_expected_layouts() {
        let segments = vec![SubtitleSrtSegment {
            start_ms: 1000,
            end_ms: 2400,
            source_text: "hello".to_string(),
            translated_text: "你好".to_string(),
        }];
        let src = build_variant_srt(&segments, ExportSrtItem::Source);
        let trans = build_variant_srt(&segments, ExportSrtItem::Target);
        let src_trans = build_variant_srt(&segments, ExportSrtItem::BilingualSourceFirst);
        let trans_src = build_variant_srt(&segments, ExportSrtItem::BilingualTargetFirst);

        assert!(src.contains("\nhello"));
        assert!(trans.contains("\n你好"));
        assert!(src_trans.contains("\nhello\n你好"));
        assert!(trans_src.contains("\n你好\nhello"));
    }

    #[test]
    fn write_variants_requires_translation_for_target_modes() {
        let dir =
            std::env::temp_dir().join(format!("voxtrans_subtitle_srt_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let segments = vec![SubtitleSrtSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: "only source".to_string(),
            translated_text: String::new(),
        }];
        let result = write_variants_to_directory(&dir, &segments, &[ExportSrtItem::Target]);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_task_output_variants_uses_export_file_names() {
        let media_dir =
            std::env::temp_dir().join(format!("voxtrans_export_media_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&media_dir);
        let media_path = media_dir.join("sample.mp3");
        let _ = std::fs::write(&media_path, []);
        let task_id = format!("export-test-{}", std::process::id());
        let segments = vec![SubtitleSrtSegment {
            start_ms: 0,
            end_ms: 1000,
            source_text: "source".to_string(),
            translated_text: "译文".to_string(),
        }];
        let items = vec![
            ExportSrtItem::Source,
            ExportSrtItem::Target,
            ExportSrtItem::BilingualSourceFirst,
            ExportSrtItem::BilingualTargetFirst,
        ];

        let result = write_task_output_variants(&task_id, &media_path, &segments, &items);
        assert!(result.is_ok());
        let paths = result.unwrap_or_default();
        assert!(paths.iter().any(|path| path.ends_with("src.srt")));
        assert!(paths.iter().any(|path| path.ends_with("trans.srt")));
        assert!(paths.iter().any(|path| path.ends_with("src_trans.srt")));
        assert!(paths.iter().any(|path| path.ends_with("trans_src.srt")));

        let output_dir = crate::services::task_path::task_output_dir(&task_id, &media_path);
        let _ = std::fs::remove_dir_all(output_dir);
        let _ = std::fs::remove_file(media_path);
        let _ = std::fs::remove_dir_all(media_dir);
    }
}
