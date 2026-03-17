use serde_json::Value;

pub(super) fn parse_progress_percent(line: &str) -> Option<u32> {
    let json_line = serde_json::from_str::<Value>(line.trim()).ok()?;
    let event_type = json_line
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let event = json_line
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if event_type == "separation" && event == "done" {
        return Some(100);
    }
    if event_type != "separation" || event != "progress" {
        return None;
    }

    let percent = if let Some(v) = json_line.get("percent").and_then(Value::as_f64) {
        v.round() as u32
    } else {
        let current = json_line
            .get("current")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let total = json_line
            .get("total")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if total > 0.0 {
            ((current / total) * 100.0).round() as u32
        } else {
            0
        }
    };

    Some(percent.clamp(0, 100))
}
