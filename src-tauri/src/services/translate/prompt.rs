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
    pub topic_summary: String,
    pub tone_strategy: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
    pub segments: Vec<TranslatePromptSegmentInput>,
}

#[derive(Debug, Clone)]
pub struct TranslateTerminologyPromptEntry {
    pub source: String,
    pub target: String,
    pub note: String,
}

pub fn build_translate_system_prompt() -> String {
    "You are a professional subtitle translator. Translate faithfully and naturally. Keep speaker tone and register. Do not add explanations. Output JSON only in this shape: {\"segments\":[{\"index\":0,\"translatedText\":\"...\"}]}".to_string()
}

#[derive(Debug, Clone)]
pub struct TranslateStylePromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub context_head: String,
    pub context_middle: String,
    pub context_tail: String,
    pub terminology_entries: Vec<TranslateTerminologyPromptEntry>,
}

pub fn build_translate_style_system_prompt() -> String {
    "You are a subtitle localization strategy planner. Return JSON only with concise strategy. Output shape: {\"topicSummary\":\"...\",\"toneStrategy\":\"...\"}".to_string()
}

pub fn build_translate_style_user_prompt(input: &TranslateStylePromptInput) -> String {
    let glossary = input
        .terminology_entries
        .iter()
        .map(terminology_entry_to_json)
        .collect::<Vec<_>>();
    let payload = serde_json::json!({
        "task": "subtitle_translation_style_planning",
        "sourceLang": input.source_lang,
        "targetLang": input.target_lang,
        "context": {
            "head": input.context_head,
            "middle": input.context_middle,
            "tail": input.context_tail
        },
        "terminology": glossary,
        "requirements": [
            "topicSummary must be 1-2 short sentences",
            "toneStrategy must be concrete and subtitle-friendly",
            "respect provided terminology"
        ],
        "output": {
            "json_only": true,
            "schema": {
                "topicSummary": "string",
                "toneStrategy": "string"
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
            "topicSummary": input.topic_summary,
            "toneStrategy": input.tone_strategy
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
            "Do not omit any segment",
            "Do not merge or split segments",
            "Do not return markdown or commentary",
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
