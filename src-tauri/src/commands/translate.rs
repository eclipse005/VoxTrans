use crate::services::translate::{
    run_translate_pipeline as run_translate_pipeline_service,
};
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTerminologyEntryCommand {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateTokenCommand {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateSegmentCommand {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatePipelineCommandRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub target_lang: String,
    pub tokens: Vec<TranslateTokenCommand>,
    #[serde(default)]
    pub translate_api_key: String,
    #[serde(default)]
    pub translate_base_url: String,
    #[serde(default)]
    pub translate_model: String,
    #[serde(default = "default_llm_concurrency")]
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_entries: Vec<TranslateTerminologyEntryCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatePipelineCommandResponse {
    pub source_srt: String,
    pub target_srt: String,
    pub bilingual_srt_source_first: String,
    pub bilingual_srt_target_first: String,
    pub segments: Vec<TranslateSegmentCommand>,
    #[serde(default)]
    pub theme_summary: String,
}

#[tauri::command]
pub async fn run_translate_pipeline(
    request: TranslatePipelineCommandRequest,
) -> Result<TranslatePipelineCommandResponse, String> {
    if request.task_id.trim().is_empty() {
        return Err("taskId is required".to_string());
    }
    if request.media_path.trim().is_empty() {
        return Err("mediaPath is required".to_string());
    }
    if request.source_lang.trim().is_empty() {
        return Err("sourceLang is required".to_string());
    }
    if request.target_lang.trim().is_empty() {
        return Err("targetLang is required".to_string());
    }
    if request.tokens.is_empty() {
        return Err("tokens is required".to_string());
    }

    let response = run_translate_pipeline_service(crate::services::translate::types::TranslatePipelineRequest {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        target_lang: request.target_lang,
        tokens: request
            .tokens
            .into_iter()
            .map(|token| crate::services::translate::types::TranslateToken {
                start: token.start,
                end: token.end,
                word: token.word,
            })
            .collect(),
        translate_api_key: request.translate_api_key,
        translate_base_url: request.translate_base_url,
        translate_model: request.translate_model,
        llm_concurrency: request.llm_concurrency,
        terminology_entries: request
            .terminology_entries
            .into_iter()
            .map(|term| crate::services::translate::types::TranslateTerminologyEntry {
                source: term.source,
                target: term.target,
                note: term.note,
            })
            .collect(),
    })
    .await?;

    Ok(TranslatePipelineCommandResponse {
        source_srt: response.source_srt,
        target_srt: response.target_srt,
        bilingual_srt_source_first: response.bilingual_srt_source_first,
        bilingual_srt_target_first: response.bilingual_srt_target_first,
        segments: response
            .segments
            .into_iter()
            .map(|segment| TranslateSegmentCommand {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                source_text: segment.source_text,
                translated_text: segment.translated_text,
            })
            .collect(),
        theme_summary: response.theme_summary,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmRequest {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranslateLlmResponse {
    pub ok: bool,
    pub message: String,
    pub model: String,
}

#[tauri::command]
pub async fn test_translate_llm(
    request: TestTranslateLlmRequest,
) -> Result<TestTranslateLlmResponse, String> {
    if request.api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if request.base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if request.model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }

    let client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.base_url.trim().to_string(),
        request.api_key.trim().to_string(),
        request.model.trim().to_string(),
    ))
    .map_err(|err| err.message)?;

    let system_prompt = "你是连通性测试助手。只返回 JSON。";
    let user_prompt = "返回 JSON：{\"ok\":true,\"message\":\"pong\"}";
    let validator = JsonResponseValidator::with_required_keys(&["ok", "message"]);
    let context = LlmCallContext {
        task_id: "settings-llm-test".to_string(),
        media_path: None,
        phase: "connectivity_test".to_string(),
    };
    let llm_id = next_llm_request_id();
    let result = client
        .call_json(
            &context,
            &llm_id,
            system_prompt,
            user_prompt,
            Some(&validator),
        )
        .await
        .map_err(|err| err.message)?;
    let ok = result
        .json
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let msg = result
        .json
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "LLM 返回缺少 message 字段".to_string())?;
    if !ok {
        return Err(format!("LLM 连通性测试失败: {msg}"));
    }
    Ok(TestTranslateLlmResponse {
        ok: true,
        message: msg.to_string(),
        model: request.model.trim().to_string(),
    })
}

fn default_llm_concurrency() -> u32 {
    4
}
