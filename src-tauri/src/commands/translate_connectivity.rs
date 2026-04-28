use super::translate_types::{TestTranslateLlmRequest, TestTranslateLlmResponse};
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use crate::services::prompts::connectivity::TRANSLATE_LLM_CONNECTIVITY_TEST;

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

    cleanup_connectivity_test_artifacts();

    let client = OpenAiCompatLlmClient::new(LlmConfig::new(
        request.base_url.trim().to_string(),
        request.api_key.trim().to_string(),
        request.model.trim().to_string(),
    ))
    .map_err(|err| err.message)?;

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
            TRANSLATE_LLM_CONNECTIVITY_TEST,
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

fn cleanup_connectivity_test_artifacts() {
    let path = crate::services::task_path::task_output_dir_by_id("settings-llm-test");
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
    }
}
