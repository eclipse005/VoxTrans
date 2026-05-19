use super::translate_types::default_llm_concurrency;

pub fn hydrate_translate_llm_settings(
    api_key: &mut String,
    base_url: &mut String,
    model: &mut String,
    llm_concurrency: &mut u32,
) -> Result<(), String> {
    if api_key.trim().is_empty()
        || base_url.trim().is_empty()
        || model.trim().is_empty()
        || *llm_concurrency == default_llm_concurrency()
    {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if api_key.trim().is_empty() {
            *api_key = settings.translate_api_key;
        }
        if base_url.trim().is_empty() {
            *base_url = settings.translate_base_url;
        }
        if model.trim().is_empty() {
            *model = settings.translate_model;
        }
        if *llm_concurrency == default_llm_concurrency() {
            *llm_concurrency = settings.llm_concurrency;
        }
    }
    if api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}

pub fn hydrate_translate_llm_connection_settings(
    api_key: &mut String,
    base_url: &mut String,
    model: &mut String,
) -> Result<(), String> {
    if api_key.trim().is_empty() || base_url.trim().is_empty() || model.trim().is_empty() {
        let settings = crate::services::preferences::load_saved_settings_from_default_path()?;
        if api_key.trim().is_empty() {
            *api_key = settings.translate_api_key;
        }
        if base_url.trim().is_empty() {
            *base_url = settings.translate_base_url;
        }
        if model.trim().is_empty() {
            *model = settings.translate_model;
        }
    }
    if api_key.trim().is_empty() {
        return Err("translateApiKey is required".to_string());
    }
    if base_url.trim().is_empty() {
        return Err("translateBaseUrl is required".to_string());
    }
    if model.trim().is_empty() {
        return Err("translateModel is required".to_string());
    }
    Ok(())
}
