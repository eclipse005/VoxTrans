use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PunctuationPromptInput {
    pub previous_text: String,
    pub current_text: String,
    pub next_text: String,
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
pub struct TranslatePromptInput {
    pub source_lang: String,
    pub target_lang: String,
    pub lines: Vec<String>,
    pub shared_prompt: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct TranslateTerminologyPromptEntry {
    #[serde(alias = "src")]
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub source: String,
    #[serde(alias = "tgt")]
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub target: String,
    #[serde(default, deserialize_with = "deserialize_string_or_empty")]
    pub note: String,
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

pub fn build_translate_summary_user_prompt(input: &TranslateSummaryPromptInput) -> String {
    let terms_section = if input.terminology_entries.is_empty() {
        String::new()
    } else {
        let terms_items = input
            .terminology_entries
            .iter()
            .map(|term| format!("- {}: {} ({})", term.source, term.target, term.note))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"
### Custom Terms List
The following is a list of custom terms with their translations:

{terms_items}

**Your task**: Select only the terms that are highly relevant to this specific video and likely to improve downstream translation, even if ASR contains recognition errors.
"#
        )
    };
    format!(
        r#"## Role
You are a video translation expert and domain analysis consultant, specializing in {source_lang} comprehension and {target_lang} expression optimization.

## Task
Analyze the {source_lang} video content and:
1. Identify the video's **domain/field** (e.g., medical, finance, technology, education, sports, entertainment, etc.)
2. Summarize the main topic in two sentences
3. From the provided custom terms list, select only terms that are **high-priority for this specific video's translation**
4. Do NOT extract new terms - only filter from the provided list

{terms_section}

## Steps
1. Domain Identification:
   - Review all three sections (front, middle, back)
   - Identify the primary domain/field of the video content
   - Examples: medical/healthcare, finance/economics, technology/IT, education, science, law, sports, entertainment, etc.

2. Topic Summary:
   - Write two sentences: first for main topic, second for key point

3. Term Selection (if custom terms provided):
   - Review each term in the custom terms list
   - Produce two ranked lists:
     - `primary_terms`: core terms that are highly likely to matter for this video's translation and should be prioritized
     - `supporting_terms`: additional related terms that are plausibly useful as context, especially when ASR may have misrecognized them
   - `primary_terms` should be compact and strict: repeated concepts, main workflow vocabulary, headline entities, and terms that are central to understanding the video
   - `supporting_terms` can be broader than `primary_terms`
   - They may include adjacent concepts, commonly co-occurring terms, and terms that would still be useful if ASR recognition is imperfect, even if they are not the video's central headline concepts
   - Prefer useful recall over excessive strictness for `supporting_terms`, but keep them meaningfully related to the video's subject matter
   - Exclude weakly related background terms, broad domain vocabulary, generic UI/product words, and everyday action words that are ambiguous in isolation
   - Do not duplicate the same term in both lists
   - Order each list from most relevant to less relevant
   - Keep `primary_terms` compact, but allow `supporting_terms` to be noticeably broader
   - If custom terms list is empty, return empty lists for both groups

## INPUT
### Front Section:
{front}

### Middle Section:
{middle}

### Back Section:
{tail}

## Output in only JSON format and no other text
```json
{{
  "theme": "Two-sentence video summary in {source_lang}",
  "primary_terms": [
    {{
      "src": "{source_lang} term (exactly as provided)",
      "tgt": "{target_lang} translation (exactly as provided)",
      "note": "Explanation (exactly as provided)"
    }}
  ],
  "supporting_terms": [
    {{
      "src": "{source_lang} term (exactly as provided)",
      "tgt": "{target_lang} translation (exactly as provided)",
      "note": "Explanation (exactly as provided)"
    }}
  ]
}}
```

Note: Start your answer with ```json and end with ```, do not add any other text."#,
        source_lang = input.source_lang,
        target_lang = input.target_lang,
        terms_section = terms_section.trim(),
        front = input.context_head,
        middle = input.context_middle,
        tail = input.context_tail
    )
}

pub fn build_translate_user_prompt(input: &TranslatePromptInput) -> String {
    let lines_joined = input.lines.join("\n");
    let json_dict = input
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            (
                (i + 1).to_string(),
                serde_json::json!({
                    "origin": line,
                    "translation": format!("{} translation {}.", input.target_lang, i + 1),
                }),
            )
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let json_format = serde_json::to_string_pretty(&serde_json::Value::Object(json_dict))
        .unwrap_or_else(|_| "{}".to_string());

    format!(
        r#"## Role
You are a professional Netflix subtitle translator, fluent in both {source_lang} and {target_lang}, as well as their respective cultures.
Your expertise lies in producing high-quality translations that are:
- Accurate and faithful to original meaning (?)
- Natural and fluent in target language expression (?)
- Appropriate in style and tone for content (?)

## Task
Translate the following {source_lang} subtitles into {target_lang} line by line.

{shared_prompt}

## Translation Principles
1. Accurately convey original meaning without arbitrary additions or omissions
2. Use idiomatic {target_lang} that flows naturally
3. Treat Stable Video Context as the global source of truth for theme and high-priority terminology
4. If a `primary_term` is relevant, use its exact `tgt` form consistently; do not paraphrase or normalize it
5. `focus_terms` are exact local term hits in the current batch and should be prioritized strongly
6. `jit_supporting_terms` are on-demand supplementary hints; use them only when they clearly match the source meaning
7. Use Dynamic Batch Context only to resolve local ambiguity from nearby lines
8. Translate each current input line only; you may reorder within a line for natural expression, but never move meaning across lines
9. Keep the number of output lines exactly the same as the number of input lines

## Input
<subtitles>
{lines}
</subtitles>

## Output in only JSON format and no other text
```json
{json_format}
```

Note: Start your answer with ```json and end with ```, do not add any other text."#,
        source_lang = input.source_lang,
        target_lang = input.target_lang,
        shared_prompt = input.shared_prompt,
        lines = lines_joined,
        json_format = json_format
    )
}

pub fn build_subtitle_split_prompt(
    language: &str,
    sentence: &str,
    suggested_parts: usize,
    word_limit: usize,
    max_parts: usize,
) -> String {
    format!(
        r#"## Role
You are a professional Netflix subtitle line splitter for **{language}**.

## Task
Split the subtitle text into the **fewest parts needed** for good subtitle readability.

Guidance:
- The suggested number of parts is **{suggested_parts}**
- Use at most **{max_parts}** parts
- Each part should ideally stay under **{word_limit}** words

Priorities:
1. Natural subtitle-sized reading units.
2. No dangling fragments or ultra-short leftovers.
3. No punctuation at the start of a new part.
4. Prefer fewer parts if they already read naturally.

Hard rules:
1. Do not rewrite or omit content.
2. Do not isolate a one-word or ultra-short fragment such as "this.", "it.", "and", or "to".
3. Punctuation marks (。！？,.!?:;) must stay at the END of a part, never at the START of the next part.
4. If no split clearly improves subtitle readability, keep the original sentence unchanged.

## Given Text
<split_this_sentence>
{sentence}
</split_this_sentence>

## Output
Return only JSON and no other text:
```json
{{
    "keep_original": false,
    "parts": [
        "part 1",
        "part 2"
    ]
}}
```

Requirements for the JSON:
- `keep_original`: true if the source sentence should remain unchanged
- `parts`: if `keep_original` is false, provide the final split parts in order

If `keep_original` is true, set `parts` to an array containing the original sentence as a single item."#
    )
}

pub fn build_align_prompt(
    source_lang: &str,
    target_lang: &str,
    src_sub: &str,
    tr_sub: &str,
    src_part: &str,
    expected_parts: usize,
) -> String {
    let src_splits = src_part.split("[br]").collect::<Vec<_>>();
    let mut align_parts_json = Vec::new();
    for i in 0..expected_parts {
        let src_value = src_splits.get(i).copied().unwrap_or("");
        align_parts_json.push(format!(
            r#"{{
            "src_part_{idx}": "{src}",
            "target_part_{idx}": "Corresponding aligned {target_lang} subtitle part"
        }}"#,
            idx = i + 1,
            src = src_value.replace('"', "\\\"")
        ));
    }
    format!(
        r#"## Role
You are a Netflix subtitle alignment expert fluent in both {source_lang} and {target_lang}.

## Task
We have {source_lang} and {target_lang} original subtitles for a Netflix program, as well as a pre-processed split version of {source_lang} subtitles.
Your task is to create the best splitting scheme for the {target_lang} subtitles based on this information.

1. Analyze the word order and structural correspondence between {source_lang} and {target_lang} subtitles
2. Split the {target_lang} subtitles into exactly the same number of parts as the pre-processed {source_lang} split version
3. Keep the {target_lang} parts balanced, natural, and comfortable to read as subtitles
4. Avoid dangling fragments, overly short leftover parts, or pushing most of the meaning into only one part
5. Never leave empty lines. If it is difficult to split literally, you may lightly rewrite for better subtitle readability while preserving meaning
6. Do not add comments or explanations in the translation, as the subtitles are for the audience to read

## INPUT
<subtitles>
{source_lang} Original: "{src_sub}"
{target_lang} Original: "{tr_sub}"
Pre-processed {source_lang} Subtitles ([br] indicates split points): {src_part}
</subtitles>

## Output in only JSON format and no other text
```json
{{
    "align": [
        {align_schema}
    ]
}}
```

Note: Start you answer with ```json and end with ```, do not add any other text."#,
        align_schema = align_parts_json.join(",\n        ")
    )
}

fn deserialize_string_or_empty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}
