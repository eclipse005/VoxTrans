use rig::providers::openai;

pub const RIG_PROVIDER: &str = "rig_openai_compatible";
pub const RIG_TRANSPORT_CHAT_COMPLETIONS: &str = "chat_completions";

pub fn build_openai_completions_client(
    api_key: &str,
    base_url: &str,
) -> Result<openai::CompletionsClient, String> {
    let mut builder = openai::Client::builder().api_key(api_key);
    let normalized_base_url = normalize_base_url(base_url);
    if !normalized_base_url.is_empty() {
        builder = builder.base_url(&normalized_base_url);
    }
    let client = builder
        .build()
        .map_err(|err| format!("failed to create rig openai client: {err}"))?;
    Ok(client.completions_api())
}

pub fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}
