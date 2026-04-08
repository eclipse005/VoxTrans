use std::path::{Path, PathBuf};
use std::process::Command;

use crate::services::binary::{configure_background_command, resolve_bundled_or_path};
use crate::services::final_subtitle::{FinalSubtitleSegment, parse_final_subtitle_segments};
use crate::services::preferences::{SubtitleLineStyle, SubtitleRenderStyle};
use crate::services::task_path::task_output_dir;

pub struct BurnHardSubtitleRequest {
    pub task_id: String,
    pub media_path: String,
    pub subtitle_segments_json: String,
    pub burn_mode: String,
    pub style: SubtitleRenderStyle,
}

pub struct BurnHardSubtitleResponse {
    pub output_path: String,
}

pub fn burn_hard_subtitle(
    request: BurnHardSubtitleRequest,
) -> Result<BurnHardSubtitleResponse, String> {
    let media_path = Path::new(request.media_path.as_str());
    let segments = parse_final_subtitle_segments(&request.subtitle_segments_json);
    if segments.is_empty() {
        return Err("当前任务没有可压制的字幕".to_string());
    }

    let lines = build_ass_dialogue_lines(&segments, request.burn_mode.trim(), &request.style);
    if lines.is_empty() {
        return Err("所选字幕类型为空，无法压制硬字幕".to_string());
    }
    let ass_text = build_ass_text(&request.style, &lines);

    let temp_ass_path = build_temp_ass_path(&request.task_id)?;
    std::fs::write(&temp_ass_path, ass_text.as_bytes()).map_err(|err| err.to_string())?;

    let output_path = task_output_dir(&request.task_id, media_path).join("burned.mp4");
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

fn build_ass_dialogue_lines(
    segments: &[FinalSubtitleSegment],
    mode: &str,
    style: &SubtitleRenderStyle,
) -> Vec<AssDialogueLine> {
    let normalized_mode = match mode {
        "source" | "target" | "bilingualSourceFirst" | "bilingualTargetFirst" => mode,
        _ => "bilingualSourceFirst",
    };
    let base_margin = style.layout.margin_v.clamp(0, 200);
    let line_gap = style.layout.bilingual_line_gap.clamp(0, 140);
    let source_upper_offset = estimate_upper_line_offset(&style.target, line_gap);
    let target_upper_offset = estimate_upper_line_offset(&style.source, line_gap);

    let mut lines = Vec::new();
    for segment in segments {
        let start_ms = segment.start_ms.max(0);
        let end_ms = segment.end_ms.max(segment.start_ms).max(0);
        let source = sanitize_ass_text(segment.source_text.trim());
        let target = sanitize_ass_text(segment.translated_text.trim());

        match normalized_mode {
            "source" => {
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
            "target" => {
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
            "bilingualTargetFirst" => {
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
            _ => {
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

    let source_style =
        build_ass_style_line("Source", &style.source, alignment, style.layout.margin_v);
    let target_style =
        build_ass_style_line("Target", &style.target, alignment, style.layout.margin_v);

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
    output.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
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
    let font_name = if font_name.is_empty() {
        "Arial"
    } else {
        font_name
    };
    let font_size = style.font_size.clamp(16, 96);
    let outline = style.outline.clamp(0.0, 8.0);
    let shadow = style.shadow.clamp(0.0, 8.0);
    let border_style = if style.border_style.trim() == "box" {
        3
    } else {
        1
    };
    let border_opacity = style.border_opacity.clamp(0, 100);
    let primary_color = hex_to_ass_color(&style.primary_color);
    let outline_color = hex_to_ass_color_with_opacity(&style.outline_color, border_opacity);
    let back_color = hex_to_ass_color_with_opacity(&style.back_color, border_opacity);
    format!(
        "Style: {name},{font_name},{font_size},{primary_color},{primary_color},{outline_color},{back_color},0,0,0,0,100,100,0,0,{border_style},{outline:.1},{shadow:.1},{alignment},46,46,{margin_v},1\n"
    )
}

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
        Err(copy_err) => execute_ffmpeg_burn(
            media_path,
            ass_path,
            output_path,
            ass_name,
            target_video_bitrate_kbps,
            false,
        )
        .map_err(|aac_err| format!("{copy_err}; 回退 AAC 后仍失败: {aac_err}")),
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

fn probe_source_video_bitrate_kbps(media_path: &Path) -> Option<u32> {
    let ffprobe_bin = resolve_bundled_or_path("ffprobe");
    let output = Command::new(ffprobe_bin)
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("stream=codec_type,bit_rate")
        .arg("-of")
        .arg("json")
        .arg(media_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let streams = value.get("streams")?.as_array()?;
    let raw_bps = streams
        .iter()
        .find(|stream| {
            stream.get("codec_type").and_then(serde_json::Value::as_str) == Some("video")
        })
        .and_then(|stream| stream.get("bit_rate"))
        .and_then(|bit_rate| {
            bit_rate
                .as_str()
                .and_then(|v| v.parse::<u64>().ok())
                .or_else(|| bit_rate.as_u64())
        })?;
    if raw_bps < 1_000 {
        return None;
    }
    let kbps = (raw_bps / 1_000) as u32;
    Some(kbps.clamp(300, 40_000))
}

fn sanitize_ass_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('{', "（")
        .replace('}', "）")
        .replace('\n', "\\N")
}

fn ass_time(ms: i64) -> String {
    let centis = (ms.max(0) / 10) as u64;
    let hours = centis / 360_000;
    let minutes = (centis % 360_000) / 6_000;
    let seconds = (centis % 6_000) / 100;
    let cs = centis % 100;
    format!("{hours}:{minutes:02}:{seconds:02}.{cs:02}")
}

fn hex_to_ass_color(raw: &str) -> String {
    hex_to_ass_color_with_opacity(raw, 100)
}

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
    let safe_id = crate::services::task_path::sanitize_filename_component(task_id);
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

struct AssDialogueLine {
    start_ms: i64,
    end_ms: i64,
    layer: u8,
    style_name: &'static str,
    margin_v: u32,
    text: String,
}
