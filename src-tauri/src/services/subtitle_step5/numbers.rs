use std::collections::HashSet;

pub(super) fn extract_numbers(text: &str) -> HashSet<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = HashSet::<String>::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let mut end = index;
            while end < chars.len()
                && (chars[end].is_ascii_digit() || chars[end] == '.' || chars[end] == ',')
            {
                end += 1;
            }
            let raw = chars[index..end].iter().collect::<String>();
            let mut value = parse_ascii_number(&raw);
            let prefix = chars[index.saturating_sub(24)..index]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            let prefix_trimmed = prefix.trim_end();
            if prefix_trimmed.ends_with("thousand and") {
                value += 1_000.0;
            } else if prefix_trimmed.ends_with("hundred and") {
                value += 100.0;
            }
            let mut next = end;
            let mut consumed_end = end;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            let has_gap = next > end;
            let has_trailing_punctuation = raw.ends_with(',')
                || raw.ends_with('.')
                || raw.ends_with('，')
                || raw.ends_with('。');
            if !has_trailing_punctuation
                && let Some((multiplier, consumed)) = parse_number_suffix(&chars[next..], has_gap) {
                    value *= multiplier;
                    consumed_end = next + consumed;
                }
            let normalized = normalize_numeric_value(value);
            if !normalized.is_empty() {
                out.insert(normalized);
            }
            index = consumed_end.max(index + 1);
            continue;
        }
        if is_chinese_number_char(chars[index]) {
            let mut end = index + 1;
            while end < chars.len() && is_chinese_number_char(chars[end]) {
                end += 1;
            }
            let raw = chars[index..end].iter().collect::<String>();
            if let Some(value) = parse_chinese_number(&raw) {
                let normalized = normalize_numeric_value(value);
                if !normalized.is_empty() {
                    out.insert(normalized);
                }
            }
            index = end;
            continue;
        }
        index += 1;
    }
    out
}

pub(super) fn parse_ascii_number(raw: &str) -> f64 {
    let cleaned = raw
        .trim_matches(|value: char| value == '.' || value == ',')
        .replace(',', "");
    cleaned.parse::<f64>().unwrap_or(0.0)
}

fn parse_number_suffix(chars: &[char], has_gap: bool) -> Option<(f64, usize)> {
    let first = chars.first().copied()?;
    if !has_gap {
        match first {
            'k' | 'K' => return Some((1_000.0, 1)),
            'm' | 'M' => return Some((1_000_000.0, 1)),
            'b' | 'B' => return Some((1_000_000_000.0, 1)),
            'w' | 'W' | '万' => return Some((10_000.0, 1)),
            '亿' => return Some((100_000_000.0, 1)),
            '千' => return Some((1_000.0, 1)),
            '百' => return Some((100.0, 1)),
            _ => {}
        }
    }
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut word = String::new();
    let mut consumed = 0usize;
    for ch in chars {
        if ch.is_ascii_alphabetic() {
            word.push(ch.to_ascii_lowercase());
            consumed += 1;
            continue;
        }
        break;
    }
    match word.as_str() {
        "k" => Some((1_000.0, consumed)),
        "m" => Some((1_000_000.0, consumed)),
        "b" => Some((1_000_000_000.0, consumed)),
        "grand" | "thousand" => Some((1_000.0, consumed)),
        "million" => Some((1_000_000.0, consumed)),
        "billion" => Some((1_000_000_000.0, consumed)),
        _ => None,
    }
}

pub(super) fn normalize_numeric_value(value: f64) -> String {
    if !value.is_finite() || value < 0.0 {
        return String::new();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 1e-6 {
        return format!("{}", rounded.round() as i64);
    }
    let text = format!("{rounded:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn is_chinese_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '〇'
            | '一'
            | '二'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '百'
            | '千'
            | '万'
            | '亿'
            | '两'
            | '壹'
            | '贰'
            | '叁'
            | '肆'
            | '伍'
            | '陆'
            | '柒'
            | '捌'
            | '玖'
            | '拾'
            | '佰'
            | '仟'
            | '第'
    )
}

fn chinese_digit_value(ch: char) -> Option<i64> {
    match ch {
        '零' | '〇' => Some(0),
        '一' | '壹' => Some(1),
        '二' | '贰' | '两' => Some(2),
        '三' | '叁' => Some(3),
        '四' | '肆' => Some(4),
        '五' | '伍' => Some(5),
        '六' | '陆' => Some(6),
        '七' | '柒' => Some(7),
        '八' | '捌' => Some(8),
        '九' | '玖' => Some(9),
        _ => None,
    }
}

fn chinese_unit_value(ch: char) -> Option<i64> {
    match ch {
        '十' | '拾' => Some(10),
        '百' | '佰' => Some(100),
        '千' | '仟' => Some(1_000),
        '万' => Some(10_000),
        '亿' => Some(100_000_000),
        _ => None,
    }
}

fn parse_chinese_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.trim_start_matches('第');
    if normalized.is_empty() {
        return None;
    }

    if normalized
        .chars()
        .all(|ch| chinese_digit_value(ch).is_some())
    {
        let mut value = 0i64;
        for ch in normalized.chars() {
            let digit = chinese_digit_value(ch)?;
            value = value.saturating_mul(10).saturating_add(digit);
        }
        return Some(value as f64);
    }

    let mut total = 0i64;
    let mut section = 0i64;
    let mut number = 0i64;
    let mut saw_numeric = false;

    for ch in normalized.chars() {
        if let Some(digit) = chinese_digit_value(ch) {
            number = digit;
            saw_numeric = true;
            continue;
        }
        let unit = chinese_unit_value(ch)?;
        saw_numeric = true;
        if unit < 10_000 {
            if number == 0 {
                number = 1;
            }
            section = section.saturating_add(number.saturating_mul(unit));
        } else {
            section = section.saturating_add(number);
            if section == 0 {
                section = 1;
            }
            total = total.saturating_add(section.saturating_mul(unit));
            section = 0;
        }
        number = 0;
    }

    if !saw_numeric {
        return None;
    }
    let value = total.saturating_add(section).saturating_add(number);
    if value <= 0 {
        return None;
    }
    Some(value as f64)
}
