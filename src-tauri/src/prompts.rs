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

#[tauri::command]
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

#[tauri::command]
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
