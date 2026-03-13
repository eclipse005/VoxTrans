#[tauri::command]
pub fn build_hotword_correction_prompts(
    request: crate::prompt_builder::BuildHotwordCorrectionPromptsRequest,
) -> Result<crate::prompt_builder::BuildHotwordCorrectionPromptsResponse, String> {
    crate::prompt_builder::build_hotword_correction_prompts(request)
}

#[tauri::command]
pub fn build_punctuation_restore_prompt(
    request: crate::prompt_builder::BuildPunctuationRestorePromptRequest,
) -> Result<crate::prompt_builder::BuildPunctuationRestorePromptResponse, String> {
    crate::prompt_builder::build_punctuation_restore_prompt(request)
}

#[tauri::command]
pub fn build_translation_profile_prompt(
    request: crate::prompt_builder::BuildTranslationProfilePromptRequest,
) -> Result<crate::prompt_builder::BuildTranslationProfilePromptResponse, String> {
    crate::prompt_builder::build_translation_profile_prompt(request)
}

#[tauri::command]
pub fn build_translation_prompt(
    request: crate::prompt_builder::BuildTranslationPromptRequest,
) -> Result<crate::prompt_builder::BuildTranslationPromptResponse, String> {
    crate::prompt_builder::build_translation_prompt(request)
}
