//! Sequential windowed extraction: the terminology core.
//!
//! One small non-streaming JSON call per window (prompt ~1-1.5K tokens — a
//! shape every endpoint, including small-TPM free tiers, can serve). Each
//! window sees the established glossary as compact context and reports only
//! NEW terms, so cross-window consistency is incremental rather than merged
//! after the fact. A width ladder (halve on failure) is built in: the system
//! never asks an endpoint twice for more than it can chew.

use serde::Deserialize;

use super::ground::split_small_windows;
use super::types::{EndReason, GlossaryEntry, TranscriptCue};
use crate::services::llm::error::LlmError;

/// Established-glossary lines carried into a window prompt.
pub(super) const ESTABLISHED_PROMPT_CAP: usize = 60;
/// Consecutive zero-new-term windows before the vocabulary is declared saturated.
pub(super) const EARLY_EXIT_EMPTY_WINDOWS: usize = 3;

/// One ASR-mishear verdict parsed from a window response.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct MisheardVerdict {
    #[serde(default)]
    pub wrong: String,
    #[serde(default)]
    pub right: String,
    #[serde(default)]
    pub note: String,
}

/// The validated shape of one window's response.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct WindowExtraction {
    #[serde(default)]
    pub terms: Vec<GlossaryEntry>,
    #[serde(default)]
    pub misheard: Vec<MisheardVerdict>,
    #[serde(default)]
    pub style_guide: String,
}

/// The LLM seam: one window extraction. Production wraps `LlmPort::call_json`
/// (retry budget, JSON repair chain, protocol reroute and usage accounting all
/// live there); tests inject scripted fakes.
#[async_trait::async_trait]
pub(super) trait TermJsonCaller: Send + Sync {
    async fn call_window(&self, request_id: &str, prompt: &str) -> Result<WindowExtraction, LlmError>;
}

pub(super) struct WindowedOutcome {
    pub glossary: Vec<GlossaryEntry>,
    pub misheard: Vec<(String, String, String)>,
    pub style_guide: String,
    pub windows_done: usize,
    pub end_reason: EndReason,
}

pub(super) struct WindowedParams<'a> {
    pub cues: &'a [TranscriptCue],
    pub user_terms: &'a [GlossaryEntry],
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub start_window_chars: usize,
    pub min_window_chars: usize,
    pub caller: &'a dyn TermJsonCaller,
    /// (windows_done, estimated_total, current_window_chars)
    pub on_progress: &'a mut (dyn FnMut(usize, usize, usize) + Send),
}

/// Run the windowed loop. Never panics on model output — malformed responses
/// surface as `LlmError` from the caller and drive the width ladder.
pub(super) async fn run_windowed_extraction(
    params: WindowedParams<'_>,
) -> WindowedOutcome {
    let WindowedParams {
        cues,
        user_terms,
        source_lang,
        target_lang,
        start_window_chars,
        min_window_chars,
        caller,
        on_progress,
    } = params;

    // Ordered glossary + key set for O(1) dedupe. Seeded with user terms so
    // windows never re-propose them. `user_seeded` counts how many actually
    // landed (duplicate keys collapse) — the output slice skips exactly that.
    let mut ordered: Vec<GlossaryEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut user_seeded = 0usize;
    for t in user_terms {
        let key = super::types::normalize_term_key(&t.source);
        if !key.is_empty() && seen.insert(key) {
            ordered.push(GlossaryEntry::new(t.source.trim(), t.target.trim(), t.note.trim()));
            user_seeded += 1;
        }
    }

    let mut misheard: Vec<(String, String, String)> = Vec::new();
    let mut style_guide = String::new();
    let mut window_chars = start_window_chars.max(min_window_chars);
    let mut windows_done = 0usize;
    let mut empty_streak = 0usize;
    let mut end_reason = EndReason::Completed;
    let mut i = 0usize;
    let mut request_seq = 0u64;
    let total_chars: usize = cues.iter().map(|c| c.text.chars().count() + 1).sum();

    while i < cues.len() {
        // Pack one window at the current width (first window of the slice).
        let (_, rel_end) = split_small_windows(&cues[i..], window_chars)[0];
        let j = i + rel_end;
        let window_text = cues[i..j].iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n");
        let window_no = windows_done + 1;

        let established = established_block(&ordered);
        let prompt = build_window_prompt(
            &window_text,
            &established,
            source_lang,
            target_lang,
            style_guide.is_empty(),
        );
        request_seq += 1;
        let request_id = format!("term-w{window_no}-{request_seq:04}");

        match caller.call_window(&request_id, &prompt).await {
            Ok(extraction) => {
                windows_done += 1;
                let mut new_terms = 0usize;
                for entry in &extraction.terms {
                    let source = entry.source.trim();
                    let target = entry.target.trim();
                    if source.is_empty() || target.is_empty() {
                        continue;
                    }
                    let key = super::types::normalize_term_key(source);
                    if key.is_empty() || seen.contains(&key) {
                        continue;
                    }
                    seen.insert(key);
                    let (target, note) = split_target_parenthetical(target, entry.note.trim());
                    ordered.push(GlossaryEntry::new(source, target, note));
                    new_terms += 1;
                }
                for pair in &extraction.misheard {
                    let wrong = pair.wrong.trim();
                    if wrong.is_empty()
                        || misheard
                            .iter()
                            .any(|(w, _, _)| w.eq_ignore_ascii_case(wrong))
                    {
                        continue;
                    }
                    misheard.push((
                        wrong.to_string(),
                        pair.right.trim().to_string(),
                        pair.note.trim().to_string(),
                    ));
                }
                if style_guide.is_empty() {
                    style_guide = extraction.style_guide.trim().to_string();
                }
                empty_streak = if new_terms == 0 { empty_streak + 1 } else { 0 };
                let window_chars_used: usize =
                    cues[i..j].iter().map(|c| c.text.chars().count() + 1).sum();
                let remaining_chars = total_chars.saturating_sub(window_chars_used);
                let total_estimate = windows_done + remaining_chars.div_ceil(window_chars.max(1));
                on_progress(windows_done, total_estimate, window_chars);
                if empty_streak >= EARLY_EXIT_EMPTY_WINDOWS {
                    end_reason = EndReason::EarlyExit;
                    break;
                }
                i = j;
            }
            Err(err) => {
                let halved = window_chars / 2;
                if halved < min_window_chars {
                    // Width floor reached: keep what we have and stop.
                    end_reason = EndReason::LlmError;
                    break;
                }
                window_chars = halved;
                empty_streak = 0;
                eprintln!(
                    "[terminology] window {window_no} failed at {0} chars ({err}); halving width to {window_chars} and retrying the same position",
                    window_text.chars().count()
                );
            }
        }
    }

    WindowedOutcome {
        glossary: ordered.into_iter().skip(user_seeded).collect(),
        misheard,
        style_guide,
        windows_done,
        end_reason,
    }
}

/// Compact established-glossary block for the prompt (first-seen order, capped).
fn established_block(ordered: &[GlossaryEntry]) -> String {
    ordered
        .iter()
        .take(ESTABLISHED_PROMPT_CAP)
        .map(|g| format!("{} -> {}", g.source, g.target))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Move a trailing parenthetical note out of the target:
/// `DOL（目标流动性）` → target `DOL`, note `目标流动性`.
/// Guards: the parenthetical must be the trailing span and ≤ 30 chars, so
/// legitimate targets like `Model 2 (2024)` with long contents stay intact.
fn split_target_parenthetical(target: &str, note: &str) -> (String, String) {
    let trimmed = target.trim();
    let close = ['）', ')'];
    if !trimmed.ends_with(close) || trimmed.chars().count() < 3 {
        return (trimmed.to_string(), note.to_string());
    }
    let close_ch = trimmed.chars().next_back().unwrap();
    let open_ch = if close_ch == '）' { '（' } else { '(' };
    let open = match trimmed.rfind(open_ch) {
        Some(pos) if pos > 0 => pos,
        _ => return (trimmed.to_string(), note.to_string()),
    };
    let inner = &trimmed[open + open_ch.len_utf8()..trimmed.len() - close_ch.len_utf8()];
    if inner.is_empty() || inner.chars().count() > 30 || inner.chars().any(|c| open_ch == c) {
        return (trimmed.to_string(), note.to_string());
    }
    let front = trimmed[..open].trim_end();
    if front.is_empty() {
        return (trimmed.to_string(), note.to_string());
    }
    let merged_note = if note.is_empty() {
        inner.to_string()
    } else {
        format!("{note}；{inner}")
    };
    (front.to_string(), merged_note)
}

fn build_window_prompt(
    window_text: &str,
    established: &str,
    source_lang: &str,
    target_lang: &str,
    first_window: bool,
) -> String {
    let style_task = if first_window {
        "\n3. 另外给出 styleGuide：这个视频译成目标语言的口吻、句式、术语处理原则（100 字内）。"
    } else {
        ""
    };
    let style_field = if first_window { ",\"styleGuide\":\"...\"" } else { "" };
    format!(
        "你是字幕术语审校。下面是视频转写的一个连续片段（{source_lang}），译文目标语言是 {target_lang}。已确定的术语表附后。\n\
任务：\n\
1. 只提取本片段中【新出现】的领域术语/专有名词/缩写——已确定术语表里已有的不要重复输出；普通常见词不要收录；只收录影响理解或会反复出现的词。target 用 {target_lang} 社区惯用译法，不要字面直译；行业缩写按惯例保留原文。note 写译法依据，没有就空字符串。\n\
2. 本片段中如有疑似语音识别误听的词（在上下文里讲不通的词），给出裁决；没有就输出空数组。{style_task}\n\
只返回 JSON：{{\"terms\":[{{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}}],\"misheard\":[{{\"wrong\":\"...\",\"right\":\"...\",\"note\":\"...\"}}]{style_field}}}\n\
\n== 已确定术语表 ==\n{}\n\
\n== 本片段转写 ==\n{}",
        if established.is_empty() { "（尚无）" } else { established },
        window_text
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn cue(index: usize, text: &str) -> TranscriptCue {
        TranscriptCue {
            index,
            start_ms: 0,
            text: text.to_string(),
        }
    }

    fn terms_json(terms: &[(&str, &str)]) -> Value {
        serde_json::json!({
            "terms": terms
                .iter()
                .map(|(s, t)| serde_json::json!({"source": s, "target": t, "note": ""}))
                .collect::<Vec<_>>(),
            "misheard": [],
        })
    }

    struct FakeCaller {
        replies: std::sync::Mutex<Vec<Result<Value, LlmError>>>,
    }
    impl FakeCaller {
        fn ok(replies: Vec<Value>) -> Self {
            Self {
                replies: std::sync::Mutex::new(replies.into_iter().map(Ok).collect()),
            }
        }
        fn with_errors_first(ok_replies: Vec<Value>, errors: usize) -> Self {
            let mut replies: Vec<Result<Value, LlmError>> =
                (0..errors).map(|_| Err(LlmError::new(crate::services::llm::error::LlmErrorKind::Http, "boom"))).collect();
            replies.extend(ok_replies.into_iter().map(Ok));
            Self { replies: std::sync::Mutex::new(replies) }
        }
    }
    #[async_trait::async_trait]
    impl TermJsonCaller for FakeCaller {
        async fn call_window(&self, _request_id: &str, _prompt: &str) -> Result<WindowExtraction, LlmError> {
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Err(LlmError::new(crate::services::llm::error::LlmErrorKind::Http, "script exhausted"));
            }
            let value = replies.remove(0)?;
            serde_json::from_value(value)
                .map_err(|e| LlmError::new(crate::services::llm::error::LlmErrorKind::InvalidSchema, e.to_string()))
        }
    }

    fn no_progress() -> Box<dyn FnMut(usize, usize, usize) + Send> {
        Box::new(|_, _, _| {})
    }

    #[tokio::test]
    async fn merges_windows_incrementally_and_dedupes() {
        // Width 30 packs one ~20-char cue per window → two windows.
        let cues = vec![cue(1, "the kill zone is hot"), cue(2, "turtle soup again")];
        let caller = FakeCaller::ok(vec![
            terms_json(&[("kill zone", "猎杀区"), ("turtle soup", "海龟汤")]),
            // window 2 re-proposes kill zone (already established) and adds TSQ
            terms_json(&[("kill zone", "杀区"), ("TSQ", "TSQ")]),
        ]);
        let mut progress = no_progress();
        let out = run_windowed_extraction(WindowedParams {
            cues: &cues,
            user_terms: &[],
            source_lang: "en",
            target_lang: "zh-CN",
            start_window_chars: 30,
            min_window_chars: 10,
            caller: &caller,
            on_progress: &mut progress,
        })
        .await;
        assert_eq!(out.glossary.len(), 3);
        assert_eq!(out.glossary[0].source, "kill zone");
        assert_eq!(out.glossary[0].target, "猎杀区", "first seen wins");
        assert_eq!(out.glossary[2].source, "TSQ");
        assert_eq!(out.end_reason, EndReason::Completed);
    }

    #[tokio::test]
    async fn early_exit_after_consecutive_empty_windows() {
        // ~37-char cues at width 300 → 8 cues/window → 4 windows for 30 cues.
        let cues: Vec<TranscriptCue> = (1..=30)
            .map(|i| cue(i, &format!("plain sentence number {i} about nothing")))
            .collect();
        // Window 1 finds one term; every later window finds nothing.
        let replies = vec![
            terms_json(&[("kill zone", "猎杀区")]),
            terms_json(&[]),
            terms_json(&[]),
            terms_json(&[]),
        ];
        let caller = FakeCaller::ok(replies);
        let mut progress = no_progress();
        let out = run_windowed_extraction(WindowedParams {
            cues: &cues,
            user_terms: &[],
            source_lang: "en",
            target_lang: "zh-CN",
            start_window_chars: 300,
            min_window_chars: 100,
            caller: &caller,
            on_progress: &mut progress,
        })
        .await;
        assert_eq!(out.end_reason, EndReason::EarlyExit);
        assert_eq!(out.windows_done, 4);
    }

    #[tokio::test]
    async fn ladder_halves_window_and_retries_same_position() {
        // First call fails twice (two halvings: 300→150→75), then succeeds.
        // 6 short cues at width 75 → 2 cues per window → 3 windows total.
        let cues: Vec<TranscriptCue> = (1..=6)
            .map(|i| cue(i, &format!("sentence {i} with kill zone")))
            .collect();
        let caller = FakeCaller::with_errors_first(
            vec![
                terms_json(&[("kill zone", "猎杀区")]),
                terms_json(&[]),
                terms_json(&[]),
            ],
            2,
        );
        let seen_widths = std::sync::Mutex::new(Vec::new());
        let mut progress = Box::new(|_done: usize, _est: usize, width: usize| {
            seen_widths.lock().unwrap().push(width);
        });
        let out = run_windowed_extraction(WindowedParams {
            cues: &cues,
            user_terms: &[],
            source_lang: "en",
            target_lang: "zh-CN",
            start_window_chars: 300,
            min_window_chars: 75,
            caller: &caller,
            on_progress: &mut progress,
        })
        .await;
        assert_eq!(out.glossary.len(), 1);
        assert_eq!(out.end_reason, EndReason::Completed);
        // Every window after the halving runs at width 75 — the ladder sticks.
        assert_eq!(*seen_widths.lock().unwrap(), vec![75, 75, 75]);
    }

    #[tokio::test]
    async fn width_floor_stops_loop_with_partial_glossary() {
        let cues: Vec<TranscriptCue> = (1..=6).map(|i| cue(i, "filler text")).collect();
        // Every call fails: ladder 400→200→100 → 50 < 100 floor aborts.
        let replies: Vec<Result<Value, LlmError>> = (0..10)
            .map(|_| Err(LlmError::new(crate::services::llm::error::LlmErrorKind::Http, "endpoint dead")))
            .collect();
        let caller = FakeCaller { replies: std::sync::Mutex::new(replies) };
        let mut progress = no_progress();
        let out = run_windowed_extraction(WindowedParams {
            cues: &cues,
            user_terms: &[],
            source_lang: "en",
            target_lang: "zh-CN",
            start_window_chars: 400,
            min_window_chars: 100,
            caller: &caller,
            on_progress: &mut progress,
        })
        .await;
        assert_eq!(out.end_reason, EndReason::LlmError);
        assert!(out.glossary.is_empty());
        assert_eq!(out.windows_done, 0);
    }

    #[tokio::test]
    async fn user_terms_are_seeded_and_never_duplicated() {
        let cues = vec![cue(1, "the kill zone is hot")];
        // Duplicate user keys collapse in the seed — the extracted-terms slice
        // must skip exactly the seeded count, not the input count.
        let user = vec![
            GlossaryEntry::new("kill zone", "猎杀区", "user"),
            GlossaryEntry::new("Kill Zone", "猎杀区", "user"),
        ];
        let caller = FakeCaller::ok(vec![terms_json(&[("kill zone", "杀区"), ("TSQ", "TSQ")])]);
        let mut progress = no_progress();
        let out = run_windowed_extraction(WindowedParams {
            cues: &cues,
            user_terms: &user,
            source_lang: "en",
            target_lang: "zh-CN",
            start_window_chars: 4000,
            min_window_chars: 1200,
            caller: &caller,
            on_progress: &mut progress,
        })
        .await;
        assert_eq!(out.glossary.len(), 1, "only the new term (TSQ) survives");
        assert_eq!(out.glossary[0].source, "TSQ");
    }

    #[test]
    fn trailing_parenthetical_moves_to_note() {
        let (target, note) = split_target_parenthetical("DOL（目标流动性）", "");
        assert_eq!(target, "DOL");
        assert_eq!(note, "目标流动性");

        let (target, note) = split_target_parenthetical("OTE（最优交易区）", "已有依据");
        assert_eq!(target, "OTE");
        assert_eq!(note, "已有依据；最优交易区");

        // No trailing parenthetical → unchanged.
        let (target, note) = split_target_parenthetical("Turtle Soup", "");
        assert_eq!(target, "Turtle Soup");
        assert_eq!(note, "");

        // Long or nested or lone contents stay intact.
        let (target, _) = split_target_parenthetical("Model 2 (2024 edition of the framework document)", "");
        assert!(target.contains("(2024"));
        let (target, _) = split_target_parenthetical("（只有括号）", "");
        assert_eq!(target, "（只有括号）");
    }

    #[test]
    fn prompt_marks_first_window_for_style_guide() {
        let first = build_window_prompt("hello world", "", "en", "zh-CN", true);
        assert!(first.contains("styleGuide"));
        let later = build_window_prompt("hello world", "A -> B", "en", "zh-CN", false);
        assert!(!later.contains("styleGuide"));
        assert!(later.contains("A -> B"));
    }
}
