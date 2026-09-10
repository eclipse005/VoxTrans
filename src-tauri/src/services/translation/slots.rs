//! Deterministic slot-level helpers for the two-stage (sentence translate +
//! slot backfill) protocol.
//!
//! Language-agnostic by construction: the code only uses Unicode-general
//! character properties (alphanumeric / whitespace) plus a tiny cross-script
//! TYPOGRAPHY set (opening brackets, per CJK 行首禁则 / JIS kinsoku backing).
//! Word-boundary knowledge for the zh target comes from the jieba dictionary
//! already shipped with the app; spaced-script splits are locally provable
//! without any dictionary. No per-language character tables live here.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use jieba_rs::{Jieba, TokenizeMode};

use super::batches::normalize_for_match;
use super::guard::lang_key;

/// Normalized core form: case-folded, fullwidth-folded, alphanumeric
/// characters only (whitespace and punctuation ignored). Two slot splits are
/// content-identical iff their core forms match — the accept gate for every
/// deterministic repair and the slot-concat validation.
pub(super) fn norm_core(s: &str) -> String {
    normalize_for_match(s)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Distribute `text` across slots proportionally to `weights` (slot source
/// char counts). Deterministic; every slot stays non-empty whenever the text
/// has at least one char per slot. Degenerate inputs (fewer chars than slots)
/// put one char in the first slots and leave the rest empty — the caller's
/// completeness check will surface that as a hard failure, as it should.
pub(super) fn proportional_split(text: &str, weights: &[usize]) -> Vec<String> {
    if weights.is_empty() {
        return Vec::new();
    }
    if weights.len() == 1 {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n < weights.len() {
        return (0..weights.len())
            .map(|i| {
                if i < n {
                    chars[i].to_string()
                } else {
                    String::new()
                }
            })
            .collect();
    }
    if weights.iter().all(|w| *w == 0) {
        // Even split by slot count when nothing is known about the slots.
        return even_split(&chars, weights.len());
    }
    // Largest-remainder apportionment: each slot's exact share of the text is
    // `n * w_i / Σw`; integer characters go to the floor, and the ones left
    // over go to the largest fractional parts (ties to the earlier slot).
    // Cumulative rounding used to decide this instead, which let the rounding
    // order — not the weights — pick the split: [1,1,1,1] over six characters
    // came out as 2/1/2/1 rather than the even 2/2/1/1.
    let total_w: f64 = weights.iter().map(|w| *w as f64).sum();
    let mut counts: Vec<usize> = Vec::with_capacity(weights.len());
    let mut remainders: Vec<(f64, usize)> = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter().enumerate() {
        let exact = n as f64 * (*weight as f64) / total_w;
        let floored = exact.floor() as usize;
        counts.push(floored);
        remainders.push((exact - floored as f64, index));
    }
    let mut leftover = n - counts.iter().sum::<usize>();
    remainders.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    for (_, index) in remainders {
        if leftover == 0 {
            break;
        }
        counts[index] += 1;
        leftover -= 1;
    }
    // Every slot keeps at least one character: a zero-weight or badly rounded
    // slot borrows from the longest one (ties take from the earlier slot).
    while let Some(empty) = counts.iter().position(|count| *count == 0) {
        let donor = counts
            .iter()
            .enumerate()
            .filter(|(index, count)| *index != empty && **count > 1)
            .max_by_key(|(index, count)| (**count, std::cmp::Reverse(*index)))
            .map(|(index, _)| index);
        let Some(donor) = donor else {
            break;
        };
        counts[donor] -= 1;
        counts[empty] += 1;
    }
    let mut out = Vec::with_capacity(counts.len());
    let mut cursor = 0usize;
    for count in counts {
        out.push(chars[cursor..cursor + count].iter().collect());
        cursor += count;
    }
    out
}

fn even_split(chars: &[char], parts: usize) -> Vec<String> {
    let n = chars.len();
    let base = n / parts;
    let rem = n % parts;
    let mut out = Vec::with_capacity(parts);
    let mut start = 0usize;
    for i in 0..parts {
        let len = base + if i < rem { 1 } else { 0 };
        out.push(chars[start..start + len].iter().collect());
        start += len;
    }
    out
}

/// Map model-returned slot texts onto the expected slot order (by global
/// segment id). `None` when any expected slot is missing or its text is empty
/// — the caller then falls back to a proportional split of the unit
/// translation.
pub(super) fn slot_texts_in_order(
    expected: &[(usize, String)],
    got: &[(usize, String)],
) -> Option<Vec<String>> {
    let by_id: HashSet<usize> = expected.iter().map(|(id, _)| *id).collect();
    let mut texts: HashMap<usize, &str> = HashMap::new();
    for (id, text) in got {
        if !by_id.contains(id) {
            continue;
        }
        if text.trim().is_empty() {
            return None;
        }
        texts.entry(*id).or_insert(text.as_str());
    }
    expected
        .iter()
        .map(|(id, _)| texts.get(id).map(|t| (*t).to_string()))
        .collect()
}

/// Deterministic boundary repair over one unit's slot texts (in place).
///
/// Detect-and-snap, with an acceptance gate — no language word tables:
/// ① a slot starting with punctuation absorbs it back into the previous slot
///    (行首禁则: closing punctuation never starts a line). Opening brackets are
///    exempt — they legitimately may start a line.
/// ② a boundary between two alphanumeric chars where at least one is ASCII
///    (spaced script) provably splits a word → snap to the nearest space.
/// ③ zh target: a boundary that falls inside a jieba word snaps to the
///    nearest dictionary word edge.
/// ④ a slot consisting entirely of punctuation pulls the preceding word
///    chunk down into itself (a line may not be made purely of punctuation;
///    downstream beautify strips bounding punctuation and would empty it).
///
/// Accept gate: the normalized concatenation must be unchanged and no slot
/// may be emptied; otherwise the original texts are kept. Moves only ever
/// shift boundary offsets (content-preserving), never rewrite words.
pub(super) fn repair_slot_boundaries(texts: &mut [String], target_lang: &str) {
    if texts.len() < 2 {
        return;
    }
    let original: Vec<String> = texts.to_vec();
    let concat: String = original.concat();
    let chars: Vec<char> = concat.chars().collect();
    let n = chars.len();
    if n == 0 {
        return;
    }
    // Original boundaries as char offsets (slots are contiguous by
    // construction: the concat IS their join).
    let mut bounds = Vec::with_capacity(texts.len() + 1);
    bounds.push(0usize);
    let mut acc = 0usize;
    for t in &original {
        acc += t.chars().count();
        bounds.push(acc);
    }

    let jieba_edges: Option<Vec<usize>> = if is_zh_target(target_lang) {
        Some(jieba_word_edges(&concat))
    } else {
        None
    };
    let attached_punct_target = is_attached_punctuation_target(target_lang);

    let mut new_bounds = bounds.clone();
    for i in 1..texts.len() {
        let prev_b = bounds[i - 1];
        let next_b = bounds[i + 1];
        // Invariants: slot i keeps ≥1 char (b < next_b), slot i-1 keeps ≥1
        // char (b > prev_b).
        let mut b = bounds[i];

        // ① Leading punctuation (excluding opening brackets) snaps right.
        while b < next_b - 1 {
            let c = chars[b];
            if !is_word_char(c) && !is_opening_punctuation(c) {
                b += 1;
            } else {
                break;
            }
        }

        // ② Provable spaced-script mid-word split → nearest space.
        if b > prev_b && b < next_b && is_word_char(chars[b - 1]) && is_word_char(chars[b]) {
            let spaced_script_involved =
                chars[b - 1].is_ascii_alphanumeric() || chars[b].is_ascii_alphanumeric();
            if spaced_script_involved {
                if let Some(space) = nearest_space(&chars, b, prev_b, next_b) {
                    b = space + 1;
                }
            } else if let Some(edges) = &jieba_edges {
                // ③ zh target: dictionary word-edge snap.
                if let Some(edge) = nearest_jieba_edge(edges, b, prev_b, next_b, &concat) {
                    b = edge;
                }
            }
        }

        // ④ Punctuation-only slot (行尾禁则 twin of rule ①): a slot whose
        // entire text is punctuation (e.g. the model parked "。" alone on a
        // slot) pulls the preceding word chunk down into itself. Runs after
        // ② so the word-edge snap cannot undo it.
        //
        // Neighbor guard: for spaced-script targets an ASCII-alphanumeric
        // char before the boundary suppresses the repair — their convention
        // parks trailing punctuation on the word, and without word-edge
        // knowledge the walk-back could split "world" into "worl|d.". CJK
        // targets (attached-punctuation typography) ignore the neighbor's
        // script: a zh line may end in an ASCII brand name ("…的 YouTube")
        // while staying a CJK line, and its trailing "。" must still be
        // absorbed — the guard keys on the TARGET's convention, never on
        // what the neighboring char happens to be.
        if (b..next_b).all(|k| !is_word_char(chars[k])) && b > prev_b {
            if attached_punct_target || !chars[b - 1].is_ascii_alphanumeric() {
                let mut target = None;
                if let Some(edges) = &jieba_edges {
                    target = edges
                        .iter()
                        .copied()
                        .filter(|e| *e > prev_b && *e < b && is_word_char(chars[*e]))
                        .min_by_key(|e| b - e);
                }
                match target {
                    Some(edge) => b = edge,
                    None => {
                        while b > prev_b + 1 && !is_word_char(chars[b]) {
                            b -= 1;
                        }
                    }
                }
            }
        }

        new_bounds[i] = b;
    }
    // Boundaries stay strictly increasing by construction (each within its
    // original (prev_b, next_b) interval); assert defensively anyway.
    for w in new_bounds.windows(2) {
        if w[0] >= w[1] {
            return;
        }
    }

    let rebuilt: Vec<String> = (0..texts.len())
        .map(|i| {
            chars[new_bounds[i]..new_bounds[i + 1]]
                .iter()
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect();
    // Accept gate: content unchanged (normalized), no empty slot.
    let old_core = norm_core(&concat);
    let new_concat: String = rebuilt.concat();
    if norm_core(&new_concat) != old_core || rebuilt.iter().any(|t| t.is_empty()) {
        return;
    }
    texts.clone_from_slice(&rebuilt);
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Cross-script opening brackets: they may legitimately start a line, so the
/// 行首禁则 snap must never pull them back. Typography standard (GB/T 15834,
/// JIS kinsoku), not a language word table.
fn is_opening_punctuation(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '（' | '［' | '｛' | '《' | '〈' | '【' | '〔' | '「' | '『'
    )
}

fn nearest_space(chars: &[char], from: usize, lo: usize, hi: usize) -> Option<usize> {
    // Nearest whitespace to `from` within [lo, hi); the boundary lands right
    // after the space (space joins the left slot), so the last usable space
    // position is hi - 2.
    let mut best: Option<usize> = None;
    for s in lo..hi.saturating_sub(1) {
        if !chars.get(s).is_some_and(|c| c.is_whitespace()) {
            continue;
        }
        let dist = s.abs_diff(from);
        if dist == 0 {
            continue; // cannot happen mid-word; guard anyway
        }
        if best.map_or(true, |b| dist < b.abs_diff(from)) {
            best = Some(s);
        }
    }
    best
}

fn is_zh_target(target_lang: &str) -> bool {
    matches!(lang_key(target_lang).as_str(), "zh" | "yue")
}

/// Targets whose typography attaches terminal punctuation to the preceding
/// word with no space (CJK family: GB/T 15834, JIS kinsoku, Korean
/// orthography). There a punctuation-only slot is always a display defect,
/// so rule ④ runs regardless of the neighbor's script. Spaced-script
/// targets keep the conservative neighbor guard: their convention parks
/// punctuation on the word, and the walk-back has no word-edge knowledge.
fn is_attached_punctuation_target(target_lang: &str) -> bool {
    matches!(lang_key(target_lang).as_str(), "zh" | "yue" | "ja" | "ko")
}

/// Char offsets of jieba dictionary word edges in `text` (excluding both text
/// ends). Punctuation and spaces are jieba tokens too, so their offsets count
/// as edges.
fn jieba_word_edges(text: &str) -> Vec<usize> {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    let jieba = JIEBA.get_or_init(Jieba::new);
    let mut byte_edges: HashSet<usize> = HashSet::new();
    for token in jieba.tokenize(text, TokenizeMode::Default, true) {
        byte_edges.insert(token.byte_end);
    }
    // Map byte offsets → char offsets. char_indices yields (byte_offset, char).
    let text_len = text.len();
    let mut edges: Vec<usize> = text
        .char_indices()
        .enumerate()
        .filter_map(|(char_offset, (byte_offset, _))| {
            if byte_offset != 0 && byte_offset != text_len && byte_edges.contains(&byte_offset) {
                Some(char_offset)
            } else {
                None
            }
        })
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
}

fn nearest_jieba_edge(
    edges: &[usize],
    from: usize,
    lo: usize,
    hi: usize,
    _concat: &str,
) -> Option<usize> {
    edges
        .iter()
        .copied()
        .filter(|edge| *edge > lo && *edge < hi)
        .min_by_key(|edge| edge.abs_diff(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_core_ignores_whitespace_and_punctuation() {
        // Same words, different spacing/punctuation → identical core.
        assert_eq!(norm_core("因此有了这期视频。"), norm_core("因此有了 这期视频"));
        assert_eq!(norm_core("Hello, world!"), norm_core("hello world"));
        assert_eq!(norm_core("Ｌａｓｔ Ｃａｌｌ。"), norm_core("last call"));
        // …but the words themselves still count: dropping one character must
        // change the core, or every "content preserved" check built on this
        // helper would be vacuous.
        assert_ne!(norm_core("因此有了这期视频"), norm_core("因此有这期视频"));
    }

    #[test]
    fn proportional_split_respects_weights_and_keeps_slots_non_empty() {
        let out = proportional_split("ABCDEFGH", &[6, 2]);
        assert_eq!(out, vec!["ABCDEF", "GH"]);
        let out = proportional_split("ABCDEF", &[1, 1, 1, 1]);
        assert_eq!(out, vec!["AB", "CD", "E", "F"]);
        // Even split when all weights are zero.
        let out = proportional_split("ABCD", &[0, 0]);
        assert_eq!(out.join(""), "ABCD");
        assert!(out.iter().all(|s| !s.is_empty()));
    }

    #[test]
    fn proportional_split_degenerate_short_text_never_panics() {
        let out = proportional_split("好", &[3, 3, 3]);
        assert_eq!(out, vec!["好", "", ""]);
    }

    #[test]
    fn slot_texts_in_order_requires_all_slots_non_empty() {
        let expected = vec![(1, "a".to_string()), (2, "b".to_string())];
        let got = vec![(2, "second".to_string()), (1, "first".to_string())];
        assert_eq!(
            slot_texts_in_order(&expected, &got),
            Some(vec!["first".to_string(), "second".to_string()])
        );
        let missing = vec![(2, "second".to_string())];
        assert_eq!(slot_texts_in_order(&expected, &missing), None);
        let empty = vec![(1, "".to_string()), (2, "second".to_string())];
        assert_eq!(slot_texts_in_order(&expected, &empty), None);
    }

    #[test]
    fn leading_punctuation_snaps_to_previous_slot() {
        // The ep6 observation: the model parked the 顿号 at the next slot's head.
        let mut texts = vec!["用2倍速一口气刷完".to_string(), "、却不真正体验".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["用2倍速一口气刷完、", "却不真正体验"]);
    }

    #[test]
    fn punctuation_only_slot_pulls_preceding_word() {
        // The ja#009 observation: ですね split across cues as です|ね。, the
        // model distributed "对吧。" as "对吧" + "。" leaving a punctuation-
        // only slot. The slot must absorb its preceding word chunk instead
        // (a line may not be made purely of punctuation).
        let mut texts = vec!["对吧".to_string(), "。".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["对", "吧。"]);
    }

    #[test]
    fn punctuation_only_slot_prefers_jieba_word_edge() {
        // concat 因此有。 — jieba edges give 因此|有|。; pulling to the
        // word edge yields 因此 + 有。 rather than 因此有 + 。.
        let mut texts = vec!["因此有".to_string(), "。".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["因此", "有。"]);
    }

    #[test]
    fn punctuation_only_slot_never_empties_previous() {
        // Previous slot holds a single char: nothing to pull without
        // emptying it, so the original split is kept.
        let mut texts = vec!["对".to_string(), "。".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["对", "。"]);
    }

    #[test]
    fn punctuation_only_slot_after_ascii_brand_repairs_for_cjk_target() {
        // A zh line may end in an ASCII brand name; the trailing "。" parked
        // alone must still be absorbed — the neighbor's script is irrelevant
        // for attached-punctuation targets (r4dcld #97 class failure).
        let mut texts = vec!["我一直都在看的 YouTube".to_string(), "。".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["我一直都在看的", "YouTube。"]);
    }

    #[test]
    fn punctuation_only_slot_after_ascii_word_stays_for_spaced_target() {
        // Spaced-script target keeps the neighbor guard: the walk-back has no
        // word-edge knowledge and must not split "world" into "worl|d.".
        let mut texts = vec!["hello world".to_string(), ".".to_string()];
        repair_slot_boundaries(&mut texts, "en");
        assert_eq!(texts, vec!["hello world", "."]);
    }

    #[test]
    fn opening_bracket_is_never_snapped() {
        let mut texts = vec!["他说".to_string(), "（笑）".to_string()];
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(texts, vec!["他说", "（笑）"]);
    }

    #[test]
    fn spaced_script_mid_word_snaps_to_nearest_space() {
        let mut texts = vec!["hel".to_string(), "lo world".to_string()];
        repair_slot_boundaries(&mut texts, "en");
        assert_eq!(texts, vec!["hello", "world"]);
    }

    #[test]
    fn zh_mid_word_snaps_to_jieba_word_edge() {
        // 因此|有|这期|视频 — a cut between 因 and 此 is inside a word; the
        // nearest edge is after 此.
        let mut texts = vec!["因".to_string(), "此有这期视频".to_string()];
        repair_slot_boundaries(&mut texts, "zh");
        assert_eq!(texts, vec!["因此", "有这期视频"]);
    }

    #[test]
    fn zh_word_aligned_boundary_is_untouched() {
        let mut texts = vec!["因此有".to_string(), "这期视频。".to_string()];
        repair_slot_boundaries(&mut texts, "zh");
        assert_eq!(texts, vec!["因此有", "这期视频。"]);
    }

    #[test]
    fn repair_preserves_concatenation() {
        let mut texts = vec![
            "第一段译文".to_string(),
            "，第二段".to_string(),
            "还有第三段！".to_string(),
        ];
        let before = norm_core(&texts.concat());
        repair_slot_boundaries(&mut texts, "zh-CN");
        assert_eq!(norm_core(&texts.concat()), before);
        assert!(texts.iter().all(|t| !t.is_empty()));
    }
}
