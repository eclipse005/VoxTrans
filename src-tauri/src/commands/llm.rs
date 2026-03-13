use tauri::State;

#[tauri::command]
pub async fn llm_interact(
    state: State<'_, crate::app_state::AppState>,
    mut request: crate::llm::LlmInteractRequest,
) -> Result<crate::llm::LlmInteractResponse, String> {
    request.usage_pool = Some(state.pool.clone());
    crate::llm::llm_interact(request).await
}

#[tauri::command]
pub async fn llm_test_connection(
    request: crate::llm::LlmTestConnectionRequest,
) -> Result<crate::llm::LlmTestConnectionResponse, String> {
    crate::llm::llm_test_connection(request).await
}
