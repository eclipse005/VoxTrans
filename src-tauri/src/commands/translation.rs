use tauri::State;

#[tauri::command]
pub async fn run_translation_pipeline(
    app_handle: tauri::AppHandle,
    state: State<'_, crate::app_state::AppState>,
    request: crate::services::translation::domain::TranslationPipelineRequest,
) -> Result<crate::services::translation::domain::TranslationPipelineResponse, String> {
    let llm_settings = state
        .llm_settings
        .read()
        .map_err(|_| "llm settings lock poisoned".to_string())?
        .clone();
    let prefs = crate::services::preferences::load_user_preferences(&state.pool).await?;
    crate::services::translation::pipeline::run_translation_pipeline(
        request,
        llm_settings,
        prefs.terms,
        state.pool.clone(),
        Some(&app_handle),
    )
    .await
}
