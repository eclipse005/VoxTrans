#[tauri::command]
pub async fn llm_interact(
    request: crate::llm::LlmInteractRequest,
) -> Result<crate::llm::LlmInteractResponse, String> {
    crate::llm::llm_interact(request).await
}

#[tauri::command]
pub async fn llm_test_connection(
    request: crate::llm::LlmTestConnectionRequest,
) -> Result<crate::llm::LlmTestConnectionResponse, String> {
    crate::llm::llm_test_connection(request).await
}
