#[derive(Debug, Clone)]
pub struct SrtCue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn normalize_cues(cues: &[SrtCue]) -> Vec<SrtCue> {
    let mut normalized: Vec<SrtCue> = cues
        .iter()
        .map(|cue| {
            let mut text = cue.text.replace("\r\n", "\n");
            text = text
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n");
            let start_ms = cue.start_ms;
            let end_ms = cue.end_ms.max(start_ms);
            SrtCue {
                index: cue.index,
                start_ms,
                end_ms,
                text: text.trim().to_string(),
            }
        })
        .collect();

    normalized.sort_by_key(|cue| cue.start_ms);
    for (idx, cue) in normalized.iter_mut().enumerate() {
        cue.index = idx + 1;
    }

    normalized
}

pub fn to_srt_from_cues(cues: &[SrtCue]) -> String {
    let normalized = normalize_cues(cues);
    if normalized.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for cue in normalized {
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_ms_to_srt_time(cue.start_ms),
            format_ms_to_srt_time(cue.end_ms)
        ));
        out.push_str(cue.text.trim());
        out.push_str("\n\n");
    }

    out
}

fn format_ms_to_srt_time(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}
