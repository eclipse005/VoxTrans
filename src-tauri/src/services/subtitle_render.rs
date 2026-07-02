//! Hard-subtitle burn-in: renders workspace segments into an ASS script and
//! bakes it into the source video with ffmpeg (`ass=` filter + libx264).
//!
//! The input is the same `WorkspaceSubtitleSegment` snapshot the subtitle
//! editor and SRT exporter consume, so transcribe-only tasks (empty
//! `translated_text`) and translate tasks (both filled) are handled by the
//! same dialogue builder — empty lines simply produce no ASS Dialogue events.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::services::binary::{configure_background_command, resolve_bundled_or_path};
use crate::services::preferences_types::{
    SubtitleBorderStyle, SubtitleBurnMode, SubtitleLineStyle, SubtitleRenderStyle,
};
use crate::services::task_path::{sanitize_filename_component, task_output_dir};
use crate::services::workspace_subtitle::WorkspaceSubtitleSegment;

pub struct BurnHardSubtitleRequest {
    pub task_id: String,
    pub media_path: String,
    pub subtitle_segments: Vec<WorkspaceSubtitleSegment>,
    pub burn_mode: SubtitleBurnMode,
    pub style: SubtitleRenderStyle,
}

pub struct BurnHardSubtitleResponse {
    pub output_path: String,
}

/// Burn the given subtitle segments into `media_path`, producing an mp4 next
/// to the other task outputs. Returns the output path on success.
pub fn burn_hard_subtitle(
    request: BurnHardSubtitleRequest,
) -> Result<BurnHardSubtitleResponse, String> {
    let media_path = Path::new(request.media_path.as_str());
    if request.subtitle_segments.is_empty() {
        return Err("当前任务没有可压制的字幕".to_string());
    }

    let lines = build_ass_dialogue_lines(&request.subtitle_segments, request.burn_mode, &request.style);
    if lines.is_empty() {
        return Err("所选字幕类型为空，无法压制硬字幕".to_string());
    }
    let ass_text = build_ass_text(&request.style, &lines);

    let temp_ass_path = build_temp_ass_path(&request.task_id)?;
    std::fs::write(&temp_ass_path, ass_text.as_bytes()).map_err(|err| err.to_string())?;

    let output_path = build_output_path(&request.task_id, media_path);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let run_result = run_ffmpeg_burn(media_path, &temp_ass_path, &output_path);
    let _ = std::fs::remove_file(&temp_ass_path);
    run_result?;

    Ok(BurnHardSubtitleResponse {
        output_path: output_path.display().to_string(),
    })
}

struct AssDialogueLine {
    start_ms: u64,
    end_ms: u64,
    layer: u8,
    style_name: &'static str,
    margin_v: u32,
    text: String,
}

fn build_ass_dialogue_lines(
    segments: &[WorkspaceSubtitleSegment],
    mode: SubtitleBurnMode,
    style: &SubtitleRenderStyle,
) -> Vec<AssDialogueLine> {
    let base_margin = style.layout.margin_v.clamp(0, 200);
    let line_gap = style.layout.bilingual_line_gap.clamp(0, 140);
    let source_upper_offset = estimate_upper_line_offset(&style.target, line_gap);
    let target_upper_offset = estimate_upper_line_offset(&style.source, line_gap);

    let mut lines = Vec::new();
    for segment in segments {
        let start_ms = segment.start_ms;
        let end_ms = segment.end_ms.max(segment.start_ms);
        let source = sanitize_ass_text(segment.source_text.trim());
        let target = sanitize_ass_text(segment.translated_text.trim());

        match mode {
            SubtitleBurnMode::Source => {
                if !source.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 0,
                        style_name: "Source",
                        margin_v: base_margin,
                        text: source,
                    });
                }
            }
            SubtitleBurnMode::Target => {
                if !target.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 0,
                        style_name: "Target",
                        margin_v: base_margin,
                        text: target,
                    });
                }
            }
            SubtitleBurnMode::BilingualTargetFirst => {
                if !target.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 1,
                        style_name: "Target",
                        margin_v: base_margin.saturating_add(target_upper_offset),
                        text: target,
                    });
                }
                if !source.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 0,
                        style_name: "Source",
                        margin_v: base_margin,
                        text: source,
                    });
                }
            }
            SubtitleBurnMode::BilingualSourceFirst => {
                if !source.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 1,
                        style_name: "Source",
                        margin_v: base_margin.saturating_add(source_upper_offset),
                        text: source,
                    });
                }
                if !target.is_empty() {
                    lines.push(AssDialogueLine {
                        start_ms,
                        end_ms,
                        layer: 0,
                        style_name: "Target",
                        margin_v: base_margin,
                        text: target,
                    });
                }
            }
        }
    }
    lines
}

/// Estimated vertical offset for the upper of two bilingual lines, so the
/// lower line (rendered at `base_margin`) and the upper line don't overlap.
fn estimate_upper_line_offset(lower_line: &SubtitleLineStyle, line_gap: u32) -> u32 {
    let font_size = lower_line.font_size.clamp(16, 96) as f64;
    let outline = lower_line.outline.clamp(0.0, 8.0);
    let shadow = lower_line.shadow.clamp(0.0, 8.0);
    let estimated_height = (font_size * 1.2 + outline * 2.0 + shadow).ceil() as u32;
    estimated_height.saturating_add(line_gap)
}

fn build_ass_text(style: &SubtitleRenderStyle, lines: &[AssDialogueLine]) -> String {
    let alignment = match style.layout.alignment {
        1..=3 => style.layout.alignment,
        _ => 2,
    };

    let source_style = build_ass_style_line("Source", &style.source, alignment, style.layout.margin_v);
    let target_style = build_ass_style_line("Target", &style.target, alignment, style.layout.margin_v);

    let mut output = String::new();
    output.push_str("[Script Info]\n");
    output.push_str("ScriptType: v4.00+\n");
    output.push_str("Collisions: Normal\n");
    output.push_str("PlayResX: 1920\n");
    output.push_str("PlayResY: 1080\n");
    output.push_str("\n[V4+ Styles]\n");
    output.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    output.push_str(&source_style);
    output.push_str(&target_style);
    output.push_str("\n[Events]\n");
    output.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for line in lines {
        output.push_str(&format!(
            "Dialogue: {},{},{},{},,0,0,{},,{}\n",
            line.layer,
            ass_time(line.start_ms),
            ass_time(line.end_ms),
            line.style_name,
            line.margin_v,
            line.text
        ));
    }
    output
}

fn build_ass_style_line(
    name: &str,
    style: &SubtitleLineStyle,
    alignment: u8,
    margin_v: u32,
) -> String {
    let font_name = style.font_family.trim();
    let font_name = if font_name.is_empty() { "Arial" } else { font_name };
    let font_size = style.font_size.clamp(16, 96);
    let outline = style.outline.clamp(0.0, 8.0);
    let shadow = style.shadow.clamp(0.0, 8.0);
    let border_style: u8 = match style.border_style {
        SubtitleBorderStyle::Box => 3,
        SubtitleBorderStyle::Outline => 1,
    };
    let border_opacity = style.border_opacity.clamp(0, 100);
    let primary_color = hex_to_ass_color(&style.primary_color);
    let outline_color = hex_to_ass_color_with_opacity(&style.outline_color, border_opacity);
    let back_color = hex_to_ass_color_with_opacity(&style.back_color, border_opacity);
    format!(
        "Style: {name},{font_name},{font_size},{primary_color},{primary_color},{outline_color},{back_color},0,0,0,0,100,100,0,0,{border_style},{outline:.1},{shadow:.1},{alignment},46,46,{margin_v},1\n"
    )
}

/// Run the burn-in. First try copying the audio stream (fast, lossless); if
/// ffmpeg rejects the source audio codec, fall back to re-encoding to AAC.
fn run_ffmpeg_burn(media_path: &Path, ass_path: &Path, output_path: &Path) -> Result<(), String> {
    let ass_name = ass_path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "ASS 临时文件名无效".to_string())?;
    let target_video_bitrate_kbps = probe_source_video_bitrate_kbps(media_path);

    match execute_ffmpeg_burn(
        media_path,
        ass_path,
        output_path,
        ass_name,
        target_video_bitrate_kbps,
        true,
    ) {
        Ok(()) => Ok(()),
        Err(copy_err) => {
            execute_ffmpeg_burn(
                media_path,
                ass_path,
                output_path,
                ass_name,
                target_video_bitrate_kbps,
                false,
            )
            .map_err(|aac_err| format!("{copy_err}; 回退 AAC 后仍失败: {aac_err}"))
        }
    }
}

fn execute_ffmpeg_burn(
    media_path: &Path,
    ass_path: &Path,
    output_path: &Path,
    ass_name: &str,
    target_video_bitrate_kbps: Option<u32>,
    audio_copy: bool,
) -> Result<(), String> {
    let ffmpeg_bin = resolve_bundled_or_path("ffmpeg");
    let mut command = Command::new(&ffmpeg_bin);
    configure_background_command(&mut command);
    if let Some(parent) = ass_path.parent() {
        command.current_dir(parent);
    }

    command
        .arg("-y")
        .arg("-i")
        .arg(media_path)
        .arg("-vf")
        .arg(format!("ass={ass_name}"))
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("medium");

    if let Some(kbps) = target_video_bitrate_kbps {
        let maxrate = kbps.saturating_mul(12) / 10;
        let bufsize = kbps.saturating_mul(2);
        command
            .arg("-b:v")
            .arg(format!("{kbps}k"))
            .arg("-maxrate")
            .arg(format!("{maxrate}k"))
            .arg("-bufsize")
            .arg(format!("{bufsize}k"));
    } else {
        command.arg("-crf").arg("23");
    }

    if audio_copy {
        command.arg("-c:a").arg("copy");
    } else {
        command.arg("-c:a").arg("aac").arg("-b:a").arg("128k");
    }

    let output = command
        .arg(output_path)
        .output()
        .map_err(|err| format!("压制硬字幕失败: 调用 ffmpeg 失败: {err}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return Err("ffmpeg 执行失败".to_string());
    }
    Err(stderr)
}

/// Probe the source video stream bitrate via `ffmpeg -i` so the burn can
/// target a comparable bitrate instead of a fixed CRF. We parse ffmpeg's own
/// stderr stream listing (the project does not ship `ffprobe`), matching the
/// convention in `frame_extract::probe_video_duration`. Returns `None` if the
/// bitrate can't be determined (caller falls back to `-crf 23`).
fn probe_source_video_bitrate_kbps(media_path: &Path) -> Option<u32> {
    let ffmpeg_bin = resolve_bundled_or_path("ffmpeg");
    let mut command = Command::new(&ffmpeg_bin);
    configure_background_command(&mut command);
    // `-i` with no output makes ffmpeg print stream info to stderr and exit
    // non-zero; we only need the stderr text.
    let output = command.arg("-i").arg(media_path).output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The video stream line looks like:
    //   "Stream #0:0...: Video: h264 ..., 1920x1080 ..., 2500 kb/s, ..."
    // The bitrate token is `, NNNN kb/s,` and only appears on the video
    // stream line. Pick the first "Video:" line, then the first kb/s token.
    let video_line = stderr.lines().find(|line| line.contains("Video:"))?;
    let kbps = video_line
        .split(',')
        .map(|part| part.trim())
        .find_map(|part| part.strip_suffix(" kb/s").and_then(|n| n.parse::<u32>().ok()))?;
    Some(kbps.clamp(300, 40_000))
}

/// Escape ASS-special characters: newlines become `\N`, braces are replaced so
/// they aren't interpreted as ASS override tags.
fn sanitize_ass_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('{', "（")
        .replace('}', "）")
        .replace('\n', "\\N")
}

/// ASS timestamp: `H:MM:SS.cc` (centiseconds, not milliseconds).
fn ass_time(ms: u64) -> String {
    let centis = ms / 10;
    let hours = centis / 360_000;
    let minutes = (centis % 360_000) / 6_000;
    let seconds = (centis % 6_000) / 100;
    let cs = centis % 100;
    format!("{hours}:{minutes:02}:{seconds:02}.{cs:02}")
}

/// Convert `#RRGGBB` to an ASS `&HAABBGGRR` colour at full opacity.
fn hex_to_ass_color(raw: &str) -> String {
    hex_to_ass_color_with_opacity(raw, 100)
}

/// Convert `#RRGGBB` to an ASS colour, applying `opacity` (0–100) to the
/// alpha channel. ASS alpha is inverted (0 = fully opaque).
fn hex_to_ass_color_with_opacity(raw: &str, opacity: u8) -> String {
    let alpha = 255_u16
        .saturating_sub((u16::from(opacity.clamp(0, 100)) * 255 + 50) / 100)
        .min(255) as u8;
    let value = raw.trim();
    if value.len() != 7 || !value.starts_with('#') {
        return format!("&H{alpha:02X}FFFFFF");
    }
    let r = &value[1..3];
    let g = &value[3..5];
    let b = &value[5..7];
    format!("&H{alpha:02X}{b}{g}{r}")
}

fn build_temp_ass_path(task_id: &str) -> Result<PathBuf, String> {
    let safe_id = sanitize_filename_component(task_id);
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let temp_dir = std::env::temp_dir().join("voxtrans");
    std::fs::create_dir_all(&temp_dir).map_err(|err| err.to_string())?;
    Ok(temp_dir.join(format!(
        "{}_{}.ass",
        if safe_id.trim().is_empty() {
            "task"
        } else {
            safe_id.as_str()
        },
        suffix
    )))
}

/// Output mp4 lives next to the SRT variants in the task output dir, named
/// after the source media so it's easy to find: `{stem}_burned.mp4`.
fn build_output_path(task_id: &str, media_path: &Path) -> PathBuf {
    let stem = media_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "output".to_string());
    let safe_stem = sanitize_filename_component(&stem);
    task_output_dir(task_id, media_path).join(format!("{safe_stem}_burned.mp4"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_segment(source: &str, target: &str) -> WorkspaceSubtitleSegment {
        WorkspaceSubtitleSegment {
            start_ms: 1_000,
            end_ms: 2_500,
            source_text: source.to_string(),
            translated_text: target.to_string(),
            source_words: Vec::new(),
        }
    }

    fn default_style() -> SubtitleRenderStyle {
        SubtitleRenderStyle::default()
    }

    #[test]
    fn ass_time_formats_centiseconds() {
        assert_eq!(ass_time(0), "0:00:00.00");
        assert_eq!(ass_time(1_000), "0:00:01.00");
        assert_eq!(ass_time(2_500), "0:00:02.50");
        assert_eq!(ass_time(3_661_001), "1:01:01.00");
    }

    #[test]
    fn sanitize_replaces_newlines_and_braces() {
        assert_eq!(sanitize_ass_text("a\nb"), "a\\Nb");
        assert_eq!(sanitize_ass_text("a{b}c"), "a（b）c");
        assert_eq!(sanitize_ass_text("a\r\nb"), "a\\Nb");
    }

    #[test]
    fn hex_color_full_opacity_white() {
        assert_eq!(hex_to_ass_color("#FFFFFF"), "&H00FFFFFF");
    }

    #[test]
    fn hex_color_swaps_rgb_to_bgr() {
        // #112233 -> BGR order 332211, alpha 00 at full opacity
        assert_eq!(hex_to_ass_color("#112233"), "&H00332211");
    }

    #[test]
    fn hex_color_half_opacity_alpha() {
        // opacity 50 -> alpha = 255 - (50*255+50)/100 = 255 - 128 = 127 = 0x7F
        assert_eq!(hex_to_ass_color_with_opacity("#FFFFFF", 50), "&H7FFFFFFF");
    }

    #[test]
    fn hex_color_invalid_falls_back_to_white() {
        assert_eq!(hex_to_ass_color("nope"), "&H00FFFFFF");
        assert_eq!(hex_to_ass_color("#123"), "&H00FFFFFF");
    }

    #[test]
    fn source_mode_skips_target_lines() {
        let segments = vec![sample_segment("Hello", "你好")];
        let lines = build_ass_dialogue_lines(&segments, SubtitleBurnMode::Source, &default_style());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].style_name, "Source");
        assert_eq!(lines[0].text, "Hello");
    }

    #[test]
    fn target_mode_skips_source_lines() {
        let segments = vec![sample_segment("Hello", "你好")];
        let lines = build_ass_dialogue_lines(&segments, SubtitleBurnMode::Target, &default_style());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].style_name, "Target");
        assert_eq!(lines[0].text, "你好");
    }

    #[test]
    fn transcribe_only_emits_source_in_bilingual_mode() {
        // translated_text empty -> bilingual still renders the source line only
        let segments = vec![sample_segment("Hello", "")];
        let lines = build_ass_dialogue_lines(
            &segments,
            SubtitleBurnMode::BilingualSourceFirst,
            &default_style(),
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].style_name, "Source");
    }

    #[test]
    fn bilingual_source_first_two_lines_with_layers() {
        let segments = vec![sample_segment("Hello", "你好")];
        let lines = build_ass_dialogue_lines(
            &segments,
            SubtitleBurnMode::BilingualSourceFirst,
            &default_style(),
        );
        assert_eq!(lines.len(), 2);
        // upper (source) on layer 1 with offset, lower (target) on layer 0 at base
        let source = lines.iter().find(|l| l.style_name == "Source").unwrap();
        let target = lines.iter().find(|l| l.style_name == "Target").unwrap();
        assert_eq!(source.layer, 1);
        assert_eq!(target.layer, 0);
        assert!(source.margin_v > target.margin_v);
    }

    #[test]
    fn empty_segments_text_produces_no_lines() {
        let segments = vec![sample_segment("", "")];
        for mode in [
            SubtitleBurnMode::Source,
            SubtitleBurnMode::Target,
            SubtitleBurnMode::BilingualSourceFirst,
            SubtitleBurnMode::BilingualTargetFirst,
        ] {
            let lines = build_ass_dialogue_lines(&segments, mode, &default_style());
            assert!(lines.is_empty(), "mode {:?} should yield no lines", mode);
        }
    }

    #[test]
    fn box_border_style_maps_to_three() {
        let mut style = default_style();
        style.source.border_style = SubtitleBorderStyle::Box;
        let line = build_ass_style_line("Source", &style.source, 2, 40);
        // border style field is the value right before outline (here 2.5)
        assert!(line.contains(",3,2.5,"), "box border should encode as 3: {line}");
    }

    #[test]
    fn outline_border_style_maps_to_one() {
        let style = default_style(); // default border_style is Outline
        let line = build_ass_style_line("Source", &style.source, 2, 40);
        assert!(line.contains(",1,2.5,"), "outline border should encode as 1: {line}");
    }

    #[test]
    fn build_output_path_uses_source_stem() {
        let media = Path::new("/tmp/My Video.mp4");
        let out = build_output_path("task-1", media);
        assert!(out.to_string_lossy().ends_with("My Video_burned.mp4"));
    }
}
