use thiserror::Error;

/// GitHub Copilot configuration errors.
///
/// Runtime provider transport/parser failures include concrete provider name and
/// endpoint path in error messages at execute-level mapping.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid provider format - expected exactly 'github-copilot', got '{0}'")]
    InvalidProviderFormat(String),

    #[error(
        "Invalid model format - expected 'backend/model' (e.g., 'anthropic/claude-sonnet-4.5', 'openai/gpt-4o', or 'openai/gpt-5.3-codex'), got '{0}'"
    )]
    InvalidModelFormat(String),

    #[error("Missing API key - set GITHUB_TOKEN or provide api_key in config")]
    MissingApiKey,

    #[error("Unknown GitHub Copilot backend: {0}. Supported backends: anthropic, openai")]
    UnknownBackend(String),

    #[error("Client creation failed: {0}")]
    ClientError(#[from] rig::http_client::Error),
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
