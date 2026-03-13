use crate::llm::{LlmTool, LlmToolFunction};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const LLM_CONNECT_TEST_SYSTEM_PROMPT: &str = "You are a connectivity checker. Reply briefly.";
pub const LLM_CONNECT_TEST_USER_PROMPT: &str = "Reply with OK.";
const HOTWORD_WINDOW_SIZE: usize = 80;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionPromptsRequest {
    pub terms: Vec<HotwordPromptTerm>,
    pub total: usize,
    pub asr_language: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HotwordPromptTerm {
    pub name: String,
    pub meaning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildHotwordCorrectionPromptsResponse {
    pub system_prompt: String,
    pub initial_task: String,
    pub tools: Vec<LlmTool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPunctuationRestorePromptRequest {
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPunctuationRestorePromptResponse {
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationProfilePromptRequest {
    pub source_language: String,
    pub target_language: String,
    pub style: Option<String>,
    pub terms: Vec<TranslationPromptTerm>,
    pub sample_texts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPromptTerm {
    pub source: String,
    pub target: String,
    pub note: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationProfilePromptResponse {
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationPromptRequest {
    pub source_language: String,
    pub target_language: String,
    pub style: Option<String>,
    pub profile_topic_summary: Option<String>,
    pub terminology_subset: Vec<TranslationPromptTerm>,
    pub shared_prompt: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildTranslationPromptResponse {
    pub system_prompt: String,
    pub user_prompt: String,
}

pub fn build_hotword_correction_prompts(
    request: BuildHotwordCorrectionPromptsRequest,
) -> Result<BuildHotwordCorrectionPromptsResponse, String> {
    let terms = dedupe_terms(request.terms);
    if terms.is_empty() {
        return Err("terms must not be empty".to_string());
    }

    let asr_language = request
        .asr_language
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("English");

    let registry = build_term_registry(&terms);
    let terms_registry = serde_json::to_string_pretty(&registry).map_err(|e| e.to_string())?;

    let system_prompt = format!(
        r#"你是一个专业的 ASR（自动语音识别）术语矫正专家。

## 任务
音频转录中常因口音、杂音将专业术语误识别为发音相近的普通词汇。你需要通过上下文分析，将这些错误还原为术语列表中的正确表达。

## 音频语言
{asr_language}

## 术语注册表
```json
{terms_registry}
```

## 核心判断方法

对每个可疑词，问自己：**快速把这个词读出来，听起来像不像术语库中的某个术语？**

如果发音相似，且上下文中该术语出现是合理的，就替换。

## ⚠️ 重要原则

1. **只矫正术语库中的术语**，不要矫正其他内容
2. **必须发音相似且上下文合理才替换**
3. **不确定时不替换**
4. **如果原文已经是术语的正确全称、正确缩写或合理复数形式，不要为了统一而改写**
5. **如果没有发现错误，直接调用 finish**

## 工作流程

1. 必须按窗口顺序覆盖全文，使用 `read_sentences(start_idx=..., end_idx=...)` 逐段阅读
2. 默认窗口大小为 {window_size} 句，先看前一窗，再继续看后一窗，直到覆盖全文
3. 通读全文后再统一决定替换，不要只看前几句就下结论
4. 逐个术语思考：这个术语的发音，在文中有没有被错误识别的形式？
5. 收集所有发现的错误对，用一次 batch_replace 完成替换
6. 调用 finish

### 批量操作示例

假设术语库中有某技术术语和某产品名称，在转录文本中发现：
- "错误形式1" 读起来像 "术语1"，且上下文在讨论相关技术
- "错误形式2" 读起来像 "术语2"，且上下文在讨论相关产品

```
batch_replace(replacements=[
  {{"old_text": "错误形式1", "new_text": "术语1"}},
  {{"old_text": "错误形式2", "new_text": "术语2"}}
])
```

### 复数形式
如果术语有复数形式出现，单数和复数各写一条规则，一次到位。"#,
        window_size = HOTWORD_WINDOW_SIZE,
    );

    let term_names = terms
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let ordered_ranges = build_ordered_ranges(request.total, HOTWORD_WINDOW_SIZE);
    let initial_task = format!(
        "请检查这 {} 个句子，找出以下术语的语音识别错误并矫正：{}\n\n请按顺序覆盖全文。建议依次阅读这些窗口：{}。\n先读取第一窗，再继续读取后续窗口，直到覆盖全文后再统一决定替换。",
        request.total,
        term_names,
        format_ranges_brief(&ordered_ranges, 10)
    );

    let tools = vec![
        LlmTool {
            r#type: "function".to_string(),
            function: LlmToolFunction {
                name: "read_sentences".to_string(),
                description: Some("读取句子内容，返回全部或指定索引范围".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "start_idx": { "type": "integer", "description": "起始索引（0基，0表示第1句）" },
                        "end_idx": { "type": "integer", "description": "结束索引（0基，开区间）" }
                    }
                }),
            },
        },
        LlmTool {
            r#type: "function".to_string(),
            function: LlmToolFunction {
                name: "batch_replace".to_string(),
                description: Some("批量执行多处替换。在所有句子中查找并替换多种错误形式".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "replacements": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_text": { "type": "string", "description": "要查找的错误文本" },
                                    "new_text": { "type": "string", "description": "替换后的正确术语" }
                                },
                                "required": ["old_text", "new_text"]
                            }
                        }
                    },
                    "required": ["replacements"]
                }),
            },
        },
        LlmTool {
            r#type: "function".to_string(),
            function: LlmToolFunction {
                name: "finish".to_string(),
                description: Some("完成矫正任务".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string", "description": "修改总结" }
                    },
                    "required": ["summary"]
                }),
            },
        },
    ];

    Ok(BuildHotwordCorrectionPromptsResponse {
        system_prompt,
        initial_task,
        tools,
    })
}

pub fn build_punctuation_restore_prompt(
    request: BuildPunctuationRestorePromptRequest,
) -> Result<BuildPunctuationRestorePromptResponse, String> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err("text must not be empty".to_string());
    }

    let system_prompt = [
        "You are an ASR punctuation restoration assistant.",
        "Only restore punctuation and capitalization.",
        "Do not add, remove, replace, or reorder words.",
        "Return strict JSON only: {\"text\":\"...\"}.",
    ]
    .join(" ");

    let user_prompt = [
        "Restore punctuation and capitalization for this ASR sentence.",
        "Keep exactly the same words and order.",
        "Output JSON only in this format: {\"text\":\"...\"}.",
        "",
        "Input text:",
        text,
    ]
    .join("\n");

    Ok(BuildPunctuationRestorePromptResponse {
        system_prompt,
        user_prompt,
    })
}

pub fn build_translation_profile_prompt(
    request: BuildTranslationProfilePromptRequest,
) -> Result<BuildTranslationProfilePromptResponse, String> {
    let source_language = request.source_language.trim();
    let target_language = request.target_language.trim();
    if source_language.is_empty() || target_language.is_empty() {
        return Err("sourceLanguage and targetLanguage are required".to_string());
    }

    let style = request
        .style
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("自然流畅、忠实原意");
    let sample_items = request
        .sample_texts
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(120)
        .collect::<Vec<_>>();
    let sample = sample_items.join("\n");
    let front = sample_items
        .iter()
        .take(40)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let middle = sample_items
        .iter()
        .skip(sample_items.len().saturating_div(2))
        .take(40)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let back = sample_items
        .iter()
        .rev()
        .take(40)
        .copied()
        .collect::<Vec<_>>();
    let back = back.into_iter().rev().collect::<Vec<_>>().join("\n");
    let terms = if request.terms.is_empty() {
        "[]".to_string()
    } else {
        serde_json::to_string(&request.terms).map_err(|e| e.to_string())?
    };

    let system_prompt =
        "You are a video translation expert and domain analysis consultant. Output JSON only."
            .to_string();

    let user_prompt = format!(
        "## Role\nYou are a video translation expert and domain analysis consultant, specializing in {source_language} comprehension and {target_language} expression optimization.\n\n## Task\nAnalyze the {source_language} video content and:\n1. Identify the video domain/field\n2. Summarize the main topic in two sentences\n3. From the provided custom terms list, select only terms that are high-priority for this specific video's translation\n4. Do NOT extract new terms - only filter from the provided list\n\n### Custom Terms List (source/target/note)\n{terms}\n\n## Steps\n1. Domain Identification from front/middle/back sections\n2. Topic Summary: exactly two sentences\n3. Term Selection:\n   - primaryTerms: highly likely to matter for this video and should be prioritized\n   - supportingTerms: additional related terms useful as context when ASR may have misrecognized terms\n   - primaryTerms should be compact and strict\n   - supportingTerms can be broader but still meaningfully related\n   - Do not duplicate terms between the two groups\n   - Keep original term objects unchanged\n\n## INPUT\n### Front Section:\n{front}\n\n### Middle Section:\n{middle}\n\n### Back Section:\n{back}\n\n### Full Sample (fallback reference):\n{sample}\n\n## Output in only JSON format and no other text\n```json\n{{\n  \"theme\": \"Two-sentence video summary in {source_language}\",\n  \"translationStyle\": \"{style}\",\n  \"primaryTerms\": [{{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}}],\n  \"supportingTerms\": [{{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}}]\n}}\n```\n\nNote: Start your answer with ```json and end with ```, do not add any other text."
    );

    Ok(BuildTranslationProfilePromptResponse {
        system_prompt,
        user_prompt,
    })
}

pub fn build_translation_prompt(
    request: BuildTranslationPromptRequest,
) -> Result<BuildTranslationPromptResponse, String> {
    let source_language = request.source_language.trim();
    let target_language = request.target_language.trim();
    if source_language.is_empty() || target_language.is_empty() {
        return Err("sourceLanguage and targetLanguage are required".to_string());
    }

    let style = request
        .style
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("自然流畅、忠实原意");
    let topic = request
        .profile_topic_summary
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("无");
    let terminology_subset =
        serde_json::to_string(&request.terminology_subset).map_err(|e| e.to_string())?;
    let lines = request
        .lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Err("lines must not be empty".to_string());
    }
    let lines_text = lines.join("\n");
    let mut output_format = serde_json::Map::new();
    for (idx, line) in lines.iter().enumerate() {
        output_format.insert(
            (idx + 1).to_string(),
            json!({
                "origin": line,
                "translation": format!("{target_language} translation {}.", idx + 1)
            }),
        );
    }
    let output_format_text =
        serde_json::to_string_pretty(&serde_json::Value::Object(output_format))
            .map_err(|e| e.to_string())?;

    let system_prompt = format!(
        "You are a professional Netflix subtitle translator, fluent in both {source_language} and {target_language}, as well as their respective cultures. Your translations must be accurate, natural, and appropriate in style and tone."
    );

    let user_prompt = format!(
        "## Task\nTranslate the following {source_language} subtitles into {target_language} line by line.\n\n{shared}\n\n## Translation Principles\n1. Accurately convey original meaning without arbitrary additions or omissions\n2. Use idiomatic {target_language} that flows naturally\n3. Treat Stable Video Context as the global source of truth for theme and high-priority terminology\n4. If a primary term is relevant, use its exact target form consistently; do not paraphrase or normalize it\n5. focus_terms are exact local term hits in the current batch and should be prioritized strongly\n6. jit_supporting_terms are on-demand supplementary hints; use them only when they clearly match the source meaning\n7. Use Dynamic Batch Context only to resolve local ambiguity from nearby lines\n8. Translate each current input line only; you may reorder within a line for natural expression, but never move meaning across lines\n9. Keep the number of output lines exactly the same as the number of input lines\n\n## Input\n<subtitles>\n{lines_text}\n</subtitles>\n\n## Metadata\n- 目标语言: {target_language}\n- 翻译风格: {style}\n- 主题摘要: {topic}\n- 术语子集: {terminology_subset}\n\n## Output in only JSON format and no other text\n```json\n{output_format_text}\n```\n\nNote: Start your answer with ```json and end with ```, do not add any other text.",
        shared = request.shared_prompt,
    );

    Ok(BuildTranslationPromptResponse {
        system_prompt,
        user_prompt,
    })
}

fn dedupe_terms(terms: Vec<HotwordPromptTerm>) -> Vec<HotwordPromptTerm> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in terms {
        let name = term.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let key = name.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        let meaning = term
            .meaning
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        out.push(HotwordPromptTerm { name, meaning });
    }
    out
}

fn looks_like_acronym(term_name: &str) -> bool {
    let letters = term_name
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<Vec<_>>();
    if letters.len() < 2 {
        return false;
    }
    let upper_letters = letters.iter().filter(|c| c.is_ascii_uppercase()).count();
    upper_letters >= std::cmp::max(2, letters.len().saturating_sub(1))
}

fn build_term_registry(terms: &[HotwordPromptTerm]) -> Vec<serde_json::Value> {
    terms
        .iter()
        .map(|term| {
            let name = term.name.trim();
            let meaning = term.meaning.as_deref().map(str::trim).filter(|v| !v.is_empty());
            json!({
                "name": name,
                "meaning": meaning,
                "canonical_form": if looks_like_acronym(name) { meaning.unwrap_or(name) } else { name },
                "is_acronym": looks_like_acronym(name),
            })
        })
        .collect()
}

fn build_ordered_ranges(total: usize, window_size: usize) -> Vec<(usize, usize)> {
    if total == 0 {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < total {
        let end = std::cmp::min(total, start + window_size);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

fn format_ranges_brief(ranges: &[(usize, usize)], max_show: usize) -> String {
    if ranges.is_empty() {
        return "[]".to_string();
    }
    let mut brief = ranges
        .iter()
        .take(max_show)
        .map(|(start, end)| format!("[{}-{}]", start, end.saturating_sub(1)))
        .collect::<Vec<_>>();
    if ranges.len() > max_show {
        brief.push("...".to_string());
    }
    brief.join(" ")
}
