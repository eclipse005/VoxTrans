use tauri::State;

#[tauri::command]
pub async fn llm_interact(
    state: State<'_, crate::app_state::AppState>,
    mut request: crate::services::llm::LlmCallEnvelope,
) -> Result<crate::services::llm::LlmInteractResponse, String> {
    request.usage_pool = Some(state.pool.clone());
    crate::services::llm::call(request).await
}

#[tauri::command]
pub async fn llm_test_connection(
    request: crate::services::llm::LlmTestConnectionRequest,
) -> Result<crate::services::llm::LlmTestConnectionResponse, String> {
    crate::services::llm::llm_test_connection(request).await
}
