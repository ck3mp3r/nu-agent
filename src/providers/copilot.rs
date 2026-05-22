use thiserror::Error;

#[derive(Debug, Error)]
pub enum CopilotError {
    #[error("Missing API key - set GITHUB_COPILOT_API_KEY or GITHUB_TOKEN, or provide api_key in config")]
    MissingApiKey,
    #[error("Client creation failed: {0}")]
    ClientError(String),
}

/// Resolve API key with fallback chain.
/// 
/// Priority order:
/// 1. Explicit `api_key` parameter
/// 2. `GITHUB_COPILOT_API_KEY` environment variable
/// 3. `GITHUB_TOKEN` environment variable
/// 
/// Returns `CopilotError::MissingApiKey` if none are available.
pub(crate) fn resolve_api_key(api_key: Option<String>) -> Result<String, CopilotError> {
    api_key
        .or_else(|| std::env::var("GITHUB_COPILOT_API_KEY").ok())
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .ok_or(CopilotError::MissingApiKey)
}

/// Create a rig copilot client from config values.
/// 
/// Uses `CopilotAuth::ApiKey` mode - the token is used as a direct bearer token,
/// NOT exchanged via GitHub's token endpoint.
/// 
/// # Arguments
/// 
/// * `api_key` - Optional explicit API key. If None, falls back to environment variables.
/// * `base_url` - Optional custom base URL for the Copilot API. If None, uses the default
///   "https://api.githubcopilot.com".
/// 
/// # Errors
/// 
/// Returns `CopilotError::MissingApiKey` if no API key is available.
/// Returns `CopilotError::ClientError` if client creation fails.
pub fn create_client(
    api_key: Option<String>,
    base_url: Option<String>,
) -> Result<rig::providers::copilot::Client, CopilotError> {
    let key = resolve_api_key(api_key)?;
    
    // Critical: Use turbofish syntax for api_key as discovered in PM-1
    // CopilotAuth implements From<S> where S: Into<String>
    let builder = rig::providers::copilot::Client::builder()
        .api_key::<rig::providers::copilot::CopilotAuth>(key);
    
    // Wire through base_url if provided
    let builder = if let Some(url) = base_url {
        builder.base_url(url)
    } else {
        builder
    };
    
    builder
        .build()
        .map_err(|e| CopilotError::ClientError(e.to_string()))
}

#[cfg(test)]
#[path = "copilot_test.rs"]
mod tests;
