#[tauri::command]
pub async fn transcribe(
    app: tauri::AppHandle,
    request: crate::services::transcribe::TranscribeRequest,
) -> Result<crate::services::transcribe::TranscribeResponse, String> {
    crate::services::transcribe::transcribe(app, request).await
}

#[tauri::command]
pub fn build_segments_from_words(
    request: crate::services::transcribe::BuildSegmentsRequest,
) -> Result<crate::services::transcribe::BuildSegmentsResponse, String> {
    crate::services::transcribe::build_segments_from_words(request)
}
