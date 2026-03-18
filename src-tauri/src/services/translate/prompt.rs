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
