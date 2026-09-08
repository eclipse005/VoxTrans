use crate::services::transcribe::WordTokenDto;
use voxtrans_core::subtitle::text_rules::{ends_with_terminal_punctuation, strip_trailing_closers};

use super::boundary_rules::{
    is_ja_turn_start_after, is_japanese_spoken_end, is_opening_punctuation,
};
use super::language::LanguageProfile;
use super::punkt_map::map_sentence_boundaries_to_word_indices;
use super::text::join_words;
use super::types::SplitReason;

/// Pre-split (hard boundaries): sentence-terminal punctuation (`. ! ? 。`).
///
/// **英文路径（Punkt 统计学习）**：当 `profile.uses_punkt_sentence_boundary()`
/// 返回 `true` 时，把所有 word 拼接成完整文本交给 Punkt 断句，再把句子边界
/// 映射回 word 索引。Punkt 会自动识别 `Mr.`/`Dr.`/`p.m.`/`U.S.` 等缩写而不
/// 在这些位置切分。单字母缩写链（`J. K.`）仍用规则特判。规则兜底只补漏掉的
/// 句号；`?`/`!` 在闭引号前或下一词不像句首时禁止硬切。
///
/// **其他语言路径（规则 + 缩写表）**：每个以 `. ! ?` 结尾的 token 产生切分，
/// 除非它在 `LanguageProfile::abbreviations()` 列表中，或是单字母缩写链。
///
/// VAD silence crossings are deliberately NOT hard-split here. Short pauses
/// are handled by the DP cost function in `subtitle_layout.rs` (only splits
/// when a span exceeds the length budget — preventing mid-sentence
/// fragmentation on short sentences that merely contain a breath pause);
/// pauses ≥ `word_units::PAUSE_SPLIT_SEC` (2s) are hard-split downstream,
/// snapped to word boundaries (speech-synced rhythm decision).
pub(super) fn build_split_points_from_hard_boundaries(
    words: &[WordTokenDto],
    profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    if profile.uses_punkt_sentence_boundary() {
        build_split_points_with_punkt(words, profile)
    } else {
        build_split_points_with_rules(words, profile)
    }
}

/// 英文路径：Punkt 统计学习断句 + 句号兜底 + 单字母缩写链特判。
///
/// Punkt 在短文本或训练数据覆盖不足时可能漏切（如 "Hello world. Again."
/// 被识别为单句）。为了不退化到比规则更差，对 Punkt 没识别的 **句号**
/// 位置再用规则补切。`?`/`!` 不走兜底：引号标题（`a "What Do You See?" post`）
/// 里 Punkt 正确地不切，强制补切会把中心语甩到下一句。
///
/// Punkt 已经切了的 `?`/`!` 也要再看一眼：闭引号后若不是句首（小写延续），
/// 仍然不切。
fn build_split_points_with_punkt(
    words: &[WordTokenDto],
    _profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    use punkt::params::Standard;
    use punkt::{SentenceTokenizer, TrainingData};
    use std::collections::HashSet;
    use std::sync::OnceLock;

    if words.is_empty() {
        return Vec::new();
    }

    // 拼接完整文本（join_words 处理标点前空格等）
    let text = join_words(words.iter().map(|w| w.word.as_str()));
    if text.trim().is_empty() {
        return Vec::new();
    }

    // 训练数据是编译时嵌入的，加载一次后复用
    static DATA: OnceLock<TrainingData> = OnceLock::new();
    let data = DATA.get_or_init(TrainingData::english);

    // Punkt 断句
    let tokenizer = SentenceTokenizer::<Standard>::new(&text, data);
    let sentences: Vec<&str> = tokenizer.collect();

    // 映射回 word 索引
    let punkt_splits: HashSet<usize> =
        map_sentence_boundaries_to_word_indices(&text, &sentences, words)
            .into_iter()
            .collect();

    // 规则兜底：只补 Punkt 漏掉的句号。排除单字母缩写链 J. K. 的内部。
    let mut out = Vec::new();
    for index in 0..words.len() {
        // Punkt 已经识别为切分点
        if punkt_splits.contains(&index) {
            // 单字母缩写链特判
            if is_single_letter_dotted(&words[index].word) {
                let continues = words
                    .get(index + 1)
                    .map(|next| is_single_letter_dotted(&next.word))
                    .unwrap_or(false);
                if continues {
                    continue;
                }
            }
            if should_suppress_question_bang_split(words, index) {
                continue;
            }
            push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
            continue;
        }

        // 规则兜底：Punkt 漏切的句号。`?`/`!` 不补。
        // 只对最后一个 word 跳过（它是真正的文本末尾，不需要切）
        if index == words.len() - 1 {
            continue;
        }
        if is_period_end_rule_fallback(&words[index].word) {
            // 单字母缩写链特判
            if is_single_letter_dotted(&words[index].word) {
                let continues = words
                    .get(index + 1)
                    .map(|next| is_single_letter_dotted(&next.word))
                    .unwrap_or(false);
                if continues {
                    continue;
                }
            }
            push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
        }
    }
    out
}

/// English `?`/`!` that is not a real sentence end: quoted title/question
/// whose next content word continues the outer sentence (`a "…?" post`),
/// or any `?`/`!` followed by a non-sentence-start (`Really? she asked`).
///
/// Closing-quote tokens are skipped when looking for the next word, so
/// `See?` + `"` + `post.` and `See?"` + `post.` behave the same.
pub(super) fn should_suppress_question_bang_split(
    words: &[WordTokenDto],
    index: usize,
) -> bool {
    let Some(token) = words.get(index) else {
        return false;
    };
    if !is_question_or_bang_terminal(&token.word) {
        return false;
    }
    let Some(next) = next_content_word(words, index) else {
        return false;
    };
    !looks_like_english_sentence_start(next)
}

fn is_question_or_bang_terminal(token: &str) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    matches!(
        normalized.chars().last(),
        Some('!' | '?' | '！' | '？' | '‼' | '⁇' | '⁈' | '⁉')
    )
}

fn is_closer_only_token(token: &str) -> bool {
    let trimmed = token.trim();
    !trimmed.is_empty() && strip_trailing_closers(trimmed).is_empty()
}

fn next_content_word(words: &[WordTokenDto], index: usize) -> Option<&str> {
    words[(index + 1)..]
        .iter()
        .map(|word| word.word.as_str())
        .find(|token| !is_closer_only_token(token))
}

fn looks_like_english_sentence_start(token: &str) -> bool {
    let trimmed = token.trim().trim_start_matches(|c: char| {
        is_opening_punctuation(c) || matches!(c, '"' | '\'' | '«' | '‹')
    });
    match trimmed.chars().find(|c| c.is_alphabetic()) {
        Some(c) => c.is_uppercase(),
        None => false,
    }
}

/// 规则兜底：token 是否以句号结尾（`.` / CJK 句号）。不含 `?`/`!`。
///
/// 这是 Punkt 漏切句号时的 fallback。缩写仍走黑名单 + 点分隔模式。
fn is_period_end_rule_fallback(token: &str) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    if normalized.is_empty() || !ends_with_period_terminal(normalized) {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    // 排除常见缩写（Punkt 可能漏切的）
    if is_common_abbreviation(&lower) {
        return false;
    }
    // 排除点分隔缩写（p.m. / U.S. / e.g. / i.e. / Ph.D.）
    if is_dotted_abbreviation(&lower) {
        return false;
    }
    true
}

fn ends_with_period_terminal(token: &str) -> bool {
    matches!(
        token.chars().last(),
        Some('.' | '。' | '｡' | '．' | '﹒')
    )
}

/// 常见英文缩写黑名单（Punkt 漏切时的兜底保护）。
///
/// 这不是完整的缩写表，只是规则兜底里用来防止误切的高频缩写。
/// Punkt 负责绝大多数缩写识别，这里只补 Punkt 训练数据覆盖不到的。
const COMMON_ABBREVIATIONS: &[&str] = &[
    "mr.", "mrs.", "ms.", "dr.", "prof.", "rev.", "hon.", "sr.", "jr.", "st.", "mt.", "no.",
    "vs.", "etc.", "al.", "cf.", "fig.", "ed.", "vol.", "pp.", "dept.", "inc.", "ltd.", "co.",
    "corp.", "bros.", "llc.", "jan.", "feb.", "mar.", "apr.", "jun.", "jul.", "aug.", "sep.",
    "sept.", "oct.", "nov.", "dec.", "ave.", "blvd.", "rd.", "ln.", "ct.", "pl.", "pres.",
    "gov.", "sen.", "rep.", "capt.", "cmdr.", "col.", "gen.", "lt.", "maj.", "sgt.", "adm.",
    "univ.", "assn.", "assoc.", "esq.", "mx.", "fr.", "amb.",
];

fn is_common_abbreviation(lower: &str) -> bool {
    COMMON_ABBREVIATIONS.contains(&lower)
}

/// 判断 token 是否是点分隔缩写模式：a.b. / u.s. / p.m. / e.g. / ph.d.
/// 形如 2+ 个短字母段用点连接，每段 <= 3 字母。
fn is_dotted_abbreviation(lower: &str) -> bool {
    // 去掉末尾的句号再判断
    let s = lower.trim_end_matches('.').trim();
    if s.is_empty() {
        return false;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_alphabetic())
    })
}

/// 其他语言路径：规则 + 缩写表。
fn build_split_points_with_rules(
    words: &[WordTokenDto],
    profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    let abbrs = profile.abbreviations();
    for index in 0..words.len() {
        let punct_end = is_terminal_end(&words[index].word, abbrs);
        let spoken_end = words.get(index + 1).is_some_and(|next| {
            let next2 = words.get(index + 2).map(|w| w.word.as_str()).unwrap_or("");
            let prev = if index == 0 {
                ""
            } else {
                words[index - 1].word.as_str()
            };
            is_japanese_spoken_end(prev, &words[index].word, &next.word, next2)
        });
        // はい / 皆さん start a new move even when the current line is short.
        let turn_before = words.get(index + 1).is_some_and(|next| {
            let next2 = words.get(index + 2).map(|w| w.word.as_str()).unwrap_or("");
            is_ja_turn_start_after(&words[index].word, &next.word, next2)
        });
        if !punct_end && !spoken_end && !turn_before {
            continue;
        }
        // Single-letter dotted token (B./A./J.): only suppress the split when
        // it forms an initial chain with the next token. An isolated single-
        // letter token is a real sentence end (e.g. "step one B.").
        if punct_end && is_single_letter_dotted(&words[index].word) {
            let continues = words
                .get(index + 1)
                .map(|next| is_single_letter_dotted(&next.word))
                .unwrap_or(false);
            if continues {
                continue;
            }
        }
        push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
    }
    out
}

/// Does `token` end with a sentence-terminal mark that should force a split?
/// Returns false for language-specific abbreviations in `abbrs`.
fn is_terminal_end(token: &str, abbrs: &[&str]) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    !abbrs.contains(&lower.as_str())
}

/// Is `token` a single ASCII letter followed by a dot (e.g. `B.`, `A.`)?
fn is_single_letter_dotted(token: &str) -> bool {
    let chars: Vec<char> = strip_trailing_closers(token.trim()).chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    use super::language::profile_for_lang;
    build_split_points_from_hard_boundaries(words, &*profile_for_lang("en"))
}

fn push_split_point(
    split_points: &mut Vec<(usize, SplitReason)>,
    index: usize,
    reason: SplitReason,
) {
    if split_points.last().map(|(end, _)| *end) == Some(index) {
        return;
    }
    split_points.push((index, reason));
}

pub(super) fn split_points_to_spans(
    word_total: usize,
    split_points: &[(usize, SplitReason)],
) -> Vec<(usize, usize)> {
    if word_total == 0 {
        return Vec::new();
    }

    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for (end, _) in split_points.iter().copied() {
        if end < cursor || end + 1 >= word_total {
            continue;
        }
        out.push((cursor, end));
        cursor = end + 1;
    }
    out.push((cursor, word_total - 1));
    out
}

/// Semantic-layer kinsoku: a sentence with NO content must not exist.
///
/// Real ASR streams occasionally exile a function-word tail into a sentence
/// of its own — a zero-duration trailing particle token (`だ` + `な。`) or
/// an odd word order after a punctuation split (`…う。` + `ます。`). Left
/// alone, such a sentence becomes a translation unit with nothing to
/// translate and renders as a bare "。" row in the SRT. A pure-tail span is
/// merged back into its predecessor, so the illegal state is unrepresentable
/// at the layer that produces sentences (the same kinsoku principle the
/// display layer already enforces on cues). Table-driven and full-match, so
/// it holds for ja / ko / zh alike and never touches content words.
pub(super) fn merge_leading_bound_particle_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    use super::boundary_rules::{is_line_start_bound_particle, strip_token};

    // A span qualifies ONLY when it is a pure function tail: every token
    // strips to bare punctuation or a line-start bound particle. Spans that
    // merely START with a particle but carry content (の三昧学で…) stay
    // untouched — those belong to the layout layer's own kinsoku balancing.
    let is_function_tail = |start: usize, end: usize| {
        (start..=end).all(|k| {
            let core = strip_token(&words[k].word);
            core.is_empty() || is_line_start_bound_particle(&words[k].word)
        })
    };

    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for &(start, end) in spans {
        let merges_back = match out.last() {
            Some(&(_, prev_end)) => start == prev_end + 1 && is_function_tail(start, end),
            None => false,
        };
        if merges_back {
            let (prev_start, _) = out.pop().expect("checked via out.last()");
            out.push((prev_start, end));
        } else {
            out.push((start, end));
        }
    }
    out
}
