use crate::services::translate::{
    adapters::rig_node::{JsonResponseValidator, RigNodeClient, RigNodeConfig},
    run_translate_pipeline as run_translate_pipeline_service,
    types::TranslatePipelineRequest,
};
use serde::{Deserialize, Serialize};

#[tauri::command]
pub async fn run_translate_pipeline(
    request: TranslatePipelineRequest,
) -> Result<crate::services::translate::types::TranslatePipelineResponse, String> {
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

    run_translate_pipeline_service(request).await
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

    let client = RigNodeClient::new(RigNodeConfig::new(
        request.base_url.trim().to_string(),
        request.api_key.trim().to_string(),
        request.model.trim().to_string(),
    ))?;

    let system_prompt = "你是连通性测试助手。只返回 JSON。";
    let user_prompt = "返回 JSON：{\"ok\":true,\"message\":\"pong\"}";
    let validator = JsonResponseValidator::with_required_keys(&["ok", "message"]);
    let result = client
        .call(
            "settings-llm-test",
            None,
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
    let model = result.model;

    Ok(TestTranslateLlmResponse {
        ok: true,
        message: msg.to_string(),
        model,
    })
}
