use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Step5PromptLine {
    pub id: usize,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step5PromptTerm {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[allow(clippy::too_many_arguments)]
pub fn build_source_split_prompt(
    source_lang: &str,
    target_lang: &str,
    full_source_text: &str,
    full_draft_translation: &str,
    source_text: &str,
    source_limit: f64,
    target_limit: f64,
    split_round: usize,
    must_split: bool,
) -> String {
    serde_json::json!({
        "task": "binary_split_source_segment_for_subtitle_alignment",
        "rule": "Think step by step internally, but output JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "fullSourceText": full_source_text,
        "fullDraftTranslation": full_draft_translation,
        "sourceText": source_text,
        "sourceLengthLimit": source_limit,
        "targetLengthLimit": target_limit,
        "splitRound": split_round,
        "mustSplit": must_split,
        "constraints": [
            "Return sourceParts only.",
            "sourceParts must be an array with either one or two strings.",
            "Use one string only when mustSplit is false and there is no natural semantic split.",
            "If mustSplit is true, return two strings.",
            "Use two strings when sourceText is too long and has a natural split point.",
            "Most sourceText values sent to this task are too long; prefer two complete chunks unless splitting would clearly damage meaning.",
            "Keep original language and wording. Do not translate.",
            "Do not reorder meaning. Keep sequence from sourceText.",
            "The strings joined together must reproduce sourceText exactly, aside from whitespace normalization.",
            "Use fullDraftTranslation only as context for semantic boundaries; do not output translation.",
            "Prefer complete clauses or sentence-like chunks over equal lengths.",
            "The length limits are soft. Never cut a word, CJK phrase, name, title, number, amount, percentage, date, or punctuation unit just to hit the limit.",
            "Avoid ultra-short fragments like single discourse markers."
        ],
        "output": {
            "sourceParts": ["part 1", "part 2"]
        }
    })
    .to_string()
}

pub fn build_align_prompt(
    source_lang: &str,
    target_lang: &str,
    theme_summary: &str,
    source_text: &str,
    draft_translation: &str,
    part_sources: &[Step5PromptLine],
    terms: &[Step5PromptTerm],
) -> String {
    serde_json::json!({
        "task": "align_translation_to_split_source_lines",
        "rule": "Return JSON only.",
        "sourceLanguage": source_lang,
        "targetLanguage": target_lang,
        "theme": theme_summary,
        "sourceText": source_text,
        "draftTranslation": draft_translation,
        "splitSourceLines": part_sources,
        "terminology": terms,
        "constraints": [
            "Return exactly one translation line for each split source line id.",
            "Keep meaning faithful and natural.",
            "Do not merge lines.",
            "Do not copy full draftTranslation to multiple ids.",
            "Each id should only contain meaning from its own source line.",
            "If uncertain, keep a shorter partial translation for that line only.",
            "Do not add explanations."
        ],
        "output": {
            "translations": [
                {"id": 1, "text": "translated text"}
            ]
        }
    })
    .to_string()
}
