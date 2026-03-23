use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PunctuationPromptInput {
    pub previous_text: String,
    pub current_text: String,
    pub next_text: String,
}

pub fn build_punctuation_system_prompt() -> String {
    "You are a subtitle punctuation normalizer. Improve punctuation, capitalization, and spacing only. Do not rewrite meaning. Do not merge neighboring sentences into the current one. Return JSON only in this shape: {\"punctuatedText\":\"...\"}.".to_string()
}

pub fn build_punctuation_user_prompt(input: &PunctuationPromptInput) -> String {
    let payload = serde_json::json!({
        "task": "punctuation_restore",
        "language": "en",
        "context": {
            "previous": input.previous_text,
            "current": input.current_text,
            "next": input.next_text
        },
        "rules": [
            "Focus on context.current only",
            "Do not add or remove semantic content",
            "Do not merge with previous or next",
            "Keep wording as close as possible",
            "Only adjust punctuation, capitalization, and spacing"
        ],
        "output": {
            "json_only": true,
            "schema": { "punctuatedText": "string" }
        }
    });
    payload.to_string()
}

#[derive(Debug, Clone)]
pub struct TranslatePromptSegmentInput {
    pub index: usize,
    pub source_text: String,
}

#[derive(Debug, Clone)]
pub struct TranslatePromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub previous_context: String,
    pub next_context: String,
    pub theme: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    pub segments: Vec<TranslatePromptSegmentInput>,
}

#[derive(Debug, Clone)]
pub struct SegmentOptimizePromptInput {
    pub preferred_mode: String,
    pub source_text: String,
    pub translated_text: String,
    pub candidate_actions: Vec<SegmentOptimizePromptCandidateInput>,
}

#[derive(Debug, Clone)]
pub struct SegmentOptimizePromptSegmentInput {
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone)]
pub struct SegmentOptimizePromptCandidateInput {
    pub candidate_id: String,
    pub action: String,
    pub timing_strategy: String,
    pub segments: Vec<SegmentOptimizePromptSegmentInput>,
    pub constraints: SegmentOptimizePromptConstraints,
}

#[derive(Debug, Clone)]
pub struct SegmentOptimizePromptConstraints {
    pub allow_split_source: bool,
    pub allow_split_target: bool,
    pub allow_reuse_target: bool,
    pub allow_proportional_timing: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct TranslateTerminologyPromptEntry {
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub source: String,
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub target: String,
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub note: String,
}

pub fn build_translate_system_prompt() -> String {
    "You are a professional subtitle translator for streaming-quality subtitles. \
Translate faithfully, naturally, and in culturally appropriate target-language phrasing. \
Preserve intent, tone, register, and key domain terminology. \
Do not add commentary or explanations. \
Output JSON only in this shape: {\"segments\":[{\"index\":1,\"translatedText\":\"...\"}]}"
        .to_string()
}

pub fn build_segment_optimize_system_prompt() -> String {
    "You are a subtitle segmentation optimizer. Decide whether the subtitle should keep its current layout or use one of the provided candidate actions. \
Return JSON only in this shape: {\"action\":\"dual_split|source_only_split|target_only_adjust|no_change\",\"candidateId\":\"...\",\"segments\":[{\"sourceText\":\"...\",\"translatedText\":\"...\"}],\"reason\":\"...\",\"confidence\":0.0}. \
If action is not no_change, candidateId must match one provided candidate action, segments must contain exactly 2 items, and the result must obey the selected candidate action's constraints. Do not add commentary."
        .to_string()
}

#[derive(Debug, Clone)]
pub struct TranslateSummaryPromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub context_head: String,
    pub context_middle: String,
    pub context_tail: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
}

pub fn build_translate_summary_system_prompt() -> String {
    "You are a subtitle translation domain analyst. \
Return JSON only in this shape: {\"theme\":\"...\",\"primaryTerminologyEntries\":[{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}],\"supportingTerminologyEntries\":[{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}]}. \
Only keep terminology that is relevant to this video's domain."
        .to_string()
}

pub fn build_translate_summary_user_prompt(input: &TranslateSummaryPromptInput) -> String {
    let glossary = input
        .terminology_entries
        .iter()
        .map(terminology_entry_to_json)
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "task": "subtitle_translation_summary_planning",
        "sourceLang": input.source_lang,
        "targetLang": input.target_lang,
        "context": {
            "head": input.context_head,
            "middle": input.context_middle,
            "tail": input.context_tail
        },
        "terminology": glossary,
        "requirements": [
            "theme must be exactly 2 sentences in source language: first sentence for main topic, second sentence for key point",
            "Select entries only from provided terminology list; do not invent new terms",
            "For selected entries, keep source/target/note exactly as provided",
            "primaryTerminologyEntries: compact and strict; repeated concepts, main workflow vocabulary, headline entities, and terms central to understanding",
            "supportingTerminologyEntries: broader same-domain terms that are plausibly useful context, especially when ASR may misrecognize terms",
            "Do not duplicate the same term in both groups",
            "Order each list from most relevant to less relevant",
            "Prefer useful recall for supporting terms, but exclude weakly related background terms and generic everyday words"
        ],
        "output": {
            "json_only": true,
            "schema": {
                "theme": "string(two sentences)",
                "primaryTerminologyEntries": [
                    {
                        "source": "string",
                        "target": "string",
                        "note": "string(optional)"
                    }
                ],
                "supportingTerminologyEntries": [
                    {
                        "source": "string",
                        "target": "string",
                        "note": "string(optional)"
                    }
                ]
            }
        }
    });
    payload.to_string()
}

pub fn build_translate_user_prompt(input: &TranslatePromptInput) -> String {
    let segments = input
        .segments
        .iter()
        .map(|segment| {
            serde_json::json!({
                "index": segment.index,
                "sourceText": segment.source_text,
            })
        })
        .collect::<Vec<_>>();

    let payload = serde_json::json!({
        "task": "subtitle_translate",
        "sourceLang": input.source_lang,
        "targetLang": input.target_lang,
        "globalStrategy": {
            "theme": input.theme
        },
        "context": {
            "previous": input.previous_context,
            "next": input.next_context,
        },
        "terminology": input
            .terminology_entries
            .iter()
            .map(terminology_entry_to_json)
            .collect::<Vec<_>>(),
        "rules": [
            "Translate only sourceText for each segment",
            "Use local segment index only (1..N in current batch), not global subtitle index",
            "Keep one output for each input index; do not omit, merge, or split segments",
            "Do not move meaning across lines; translate each input line independently",
            "Preserve meaning and speaker intent; do not add facts or commentary",
            "Use natural subtitle phrasing in target language",
            "Conciseness-first: avoid unnecessary expansion; if multiple faithful options exist, prefer the shorter one",
            "Use provided terminology consistently",
            "Use previous/next context only for disambiguation",
            "Return JSON only"
        ],
        "segments": segments,
        "output": {
            "json_only": true,
            "schema": {
                "segments": [
                    {
                        "index": "number",
                        "translatedText": "string"
                    }
                ]
            }
        }
    });

    payload.to_string()
}

pub fn build_segment_optimize_user_prompt(input: &SegmentOptimizePromptInput) -> String {
    let payload = serde_json::json!({
        "task": "subtitle_segment_optimize",
        "preferredMode": input.preferred_mode,
        "original": {
            "sourceText": input.source_text,
            "translatedText": input.translated_text,
        },
        "candidateActions": input.candidate_actions.iter().map(|candidate| {
            serde_json::json!({
                "candidateId": candidate.candidate_id,
                "action": candidate.action,
                "timingStrategy": candidate.timing_strategy,
                "segments": candidate.segments.iter().map(|segment| {
                    serde_json::json!({
                        "sourceText": segment.source_text,
                        "translatedText": segment.translated_text,
                    })
                }).collect::<Vec<_>>(),
                "constraints": {
                    "allowSplitSource": candidate.constraints.allow_split_source,
                    "allowSplitTarget": candidate.constraints.allow_split_target,
                    "allowReuseTarget": candidate.constraints.allow_reuse_target,
                    "allowProportionalTiming": candidate.constraints.allow_proportional_timing,
                }
            })
        }).collect::<Vec<_>>(),
        "rules": [
            "Choose action=no_change if none of the candidate actions are safe and natural",
            "If you choose a candidate action, candidateId must exactly match one candidateActions.candidateId value",
            "action must match the selected candidate's action",
            "Preserve meaning; do not add or remove information",
            "Prefer semantically complete segments",
            "Keep subtitle reading flow natural and watchable",
            "Respect the selected candidate action's constraints strictly",
            "If action is not no_change, return exactly 2 segments",
            "Keep sourceText and translatedText non-empty for every returned segment",
            "Only adjust split boundaries; do not rewrite beyond what is needed for split alignment"
        ],
        "output": {
            "json_only": true,
            "schema": {
                "action": "dual_split|source_only_split|target_only_adjust|no_change",
                "candidateId": "string(empty only when action=no_change)",
                "segments": [
                    {
                        "sourceText": "string",
                        "translatedText": "string"
                    }
                ],
                "reason": "string",
                "confidence": "number(0..1)"
            }
        }
    });
    payload.to_string()
}

fn terminology_entry_to_json(entry: &TranslateTerminologyPromptEntry) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("source".to_string(), serde_json::Value::String(entry.source.clone()));
    map.insert("target".to_string(), serde_json::Value::String(entry.target.clone()));
    let note = entry.note.trim();
    if !note.is_empty() {
        map.insert("note".to_string(), serde_json::Value::String(note.to_string()));
    }
    serde_json::Value::Object(map)
}

fn deserialize_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}
