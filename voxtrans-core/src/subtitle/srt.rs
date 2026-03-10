use parakeet_rs::TimedToken;

pub fn to_srt_from_sentence_tokens(tokens: &[TimedToken]) -> String {
    if tokens.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut index = 1usize;

    for token in tokens {
        let text = token.text.trim();
        if text.is_empty() {
            continue;
        }

        let start = format_srt_time(token.start.max(0.0));
        let end = format_srt_time(token.end.max(token.start));
        out.push_str(&index.to_string());
        out.push('\n');
        out.push_str(&format!("{start} --> {end}\n"));
        out.push_str(text);
        out.push_str("\n\n");
        index += 1;
    }

    out
}

fn format_srt_time(seconds: f32) -> String {
    let total_ms = (seconds as f64 * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02},{millis:03}")
}
