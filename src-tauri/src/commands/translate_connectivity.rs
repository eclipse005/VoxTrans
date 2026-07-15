use super::translate_types::{
    ListLlmModelsRequest, ListLlmModelsResponse, LlmModelInfoDto, TestTranslateLlmRequest,
    TestTranslateLlmResponse,
};
use base64::Engine;
use crate::services::llm::client::OpenAiCompatLlmClient;
use crate::services::llm::json_guard::JsonResponseValidator;
use crate::services::llm::port::{LlmCallContext, LlmConfig, LlmPort, next_llm_request_id};
use crate::services::prompts::connectivity::{
    TRANSLATE_LLM_CONNECTIVITY_TEST, TRANSLATE_LLM_CONNECTIVITY_TEST_VISION,
    VISION_PROBE_IMAGE_BYTES,
};
use serde_json::Value;

#[tauri::command]
pub async fn test_translate_llm(
    request: TestTranslateLlmRequest,
) -> Result<TestTranslateLlmResponse, String> {
    // Local providers (Ollama) may send a placeholder key.
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

/// GET `{baseUrl}/models` (OpenAI-compatible). Used by settings to populate the model picker.
#[tauri::command]
pub async fn list_llm_models(
    request: ListLlmModelsRequest,
) -> Result<ListLlmModelsResponse, String> {
    let base = request.base_url.trim().trim_end_matches('/').to_string();
    let api_key = request.api_key.trim();
    if api_key.is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if base.is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }

    let url = format!("{base}/models");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let response = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| format!("list models request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let msg = parse_error_message(&body).unwrap_or_else(|| format!("HTTP {status}"));
        return Err(msg);
    }

    let payload: Value = response
        .json()
        .await
        .map_err(|e| format!("list models parse failed: {e}"))?;

    let mut all = extract_models(&payload);
    // Prefer latest/common models first (providers often return ascending).
    all.reverse();

    if all.is_empty() {
        return Err("接口已响应，但未解析到模型列表，请手填模型名".to_string());
    }

    let chat_models: Vec<_> = all.iter().filter(|m| m.kind == "chat").cloned().collect();
    let excluded_models: Vec<_> = all.iter().filter(|m| m.kind != "chat").cloned().collect();

    Ok(ListLlmModelsResponse {
        chat_models,
        excluded_models,
        all_models: all,
    })
}

fn parse_error_message(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .or_else(|| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
}

fn extract_models(payload: &Value) -> Vec<LlmModelInfoDto> {
    let items = payload
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| payload.get("models").and_then(|d| d.as_array()));

    let Some(items) = items else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let id = if let Some(s) = item.as_str() {
            s.trim().to_string()
        } else if let Some(obj) = item.as_object() {
            obj.get("id")
                .or_else(|| obj.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            String::new()
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        out.push(LlmModelInfoDto {
            kind: classify_model_kind(&id).to_string(),
            id,
        });
    }
    out
}

fn classify_model_kind(id: &str) -> &'static str {
    let s = id.to_ascii_lowercase();
    // Chat/multimodal dialogue models first — avoid excluding gpt-*-audio etc.
    if looks_like_chat(&s) {
        return "chat";
    }
    if regex_like_image(&s) {
        return "image";
    }
    if regex_like_video(&s) {
        return "video";
    }
    if regex_like_audio(&s) {
        return "audio";
    }
    if s.contains("embedding") || s.contains("embed") || s.contains("bge-") || s.contains("e5-") {
        return "embedding";
    }
    if s.contains("moderation") || s.contains("rerank") || s.contains("classify") {
        return "other";
    }
    "chat"
}

fn looks_like_chat(s: &str) -> bool {
    s.contains("gpt")
        || s.contains("claude")
        || s.contains("deepseek")
        || s.contains("qwen")
        || s.contains("gemini")
        || s.contains("glm")
        || s.contains("doubao")
        || s.contains("llama")
        || s.contains("mistral")
        || s.contains("chat")
        || s.contains("instruct")
}

fn regex_like_image(s: &str) -> bool {
    s.contains("dall-e")
        || s.contains("dalle")
        || s.contains("flux")
        || s.contains("sdxl")
        || s.contains("stable-diffusion")
        || s.contains("seedream")
        || s.contains("imagen")
        || s.contains("text-to-image")
        || s.contains("t2i")
}

fn regex_like_video(s: &str) -> bool {
    s.contains("seedance")
        || s.contains("sora")
        || s.contains("kling")
        || s.contains("runway")
        || s.contains("text-to-video")
        || s.contains("t2v")
        || s.contains("i2v")
}

fn regex_like_audio(s: &str) -> bool {
    // Prefer explicit non-chat audio products; bare "speech"/"-audio" is too broad.
    s.contains("tts")
        || s.contains("whisper")
        || s.contains("text-to-speech")
        || s.contains("speech-to-text")
        || (s.contains("asr") && !looks_like_chat(s))
}
