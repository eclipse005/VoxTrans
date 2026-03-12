#[tauri::command]
pub fn build_hotword_correction_prompts(
    request: crate::prompts::BuildHotwordCorrectionPromptsRequest,
) -> Result<crate::prompts::BuildHotwordCorrectionPromptsResponse, String> {
    crate::prompts::build_hotword_correction_prompts(request)
}

#[tauri::command]
pub fn build_punctuation_restore_prompt(
    request: crate::prompts::BuildPunctuationRestorePromptRequest,
) -> Result<crate::prompts::BuildPunctuationRestorePromptResponse, String> {
    crate::prompts::build_punctuation_restore_prompt(request)
}
