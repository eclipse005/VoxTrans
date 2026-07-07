use super::translate_types::{TestTranslateLlmRequest, TestTranslateLlmResponse};
use base64::Engine;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use crate::services::prompts::connectivity::{
    TRANSLATE_LLM_CONNECTIVITY_TEST, TRANSLATE_LLM_CONNECTIVITY_TEST_VISION,
    VISION_PROBE_IMAGE_BYTES,
};

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
    ))?;

    let validator = JsonResponseValidator::with_required_keys(&["ok", "message"]);
    let context = LlmCallContext {
        task_id: "settings-llm-test".to_string(),
        media_path: None,
        phase: "connectivity_test".to_string(),
        store: None,
    };
    let llm_id = next_llm_request_id();

    // When vision assist is enabled in settings, probe with an attached image
    // so we detect early whether the configured model actually supports image
    // input. A text-only model will 4xx/5xx or return a non-JSON response,
    // which surfaces as a clear error before the user commits to a full run.
    let (prompt, images): (&str, Option<Vec<String>>) = if request.enable_vision_assist {
        let b64 = base64::engine::general_purpose::STANDARD.encode(VISION_PROBE_IMAGE_BYTES);
        let data_url = format!("data:image/jpeg;base64,{b64}");
        (
            TRANSLATE_LLM_CONNECTIVITY_TEST_VISION,
            Some(vec![data_url]),
        )
    } else {
        (TRANSLATE_LLM_CONNECTIVITY_TEST, None)
    };

    let result = client
        .call_json(
            &context,
            &llm_id,
            prompt,
            images.as_deref(),
            Some(&validator),
        )
        .await
        .map_err(|err| {
            if request.enable_vision_assist {
                format!(
                    "LLM connectivity test failed (vision assist enabled): {}. If the model does not support image input, turn off the vision assist toggle.",
                    err.message
                )
            } else {
                format!("LLM connectivity test failed: {}", err.message)
            }
        })?;
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
        .ok_or_else(|| "LLM response missing 'message' field".to_string())?;
    if !ok {
        return Err(format!("LLM connectivity test failed: {msg}"));
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
