use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid provider format - expected exactly 'github-copilot', got '{0}'")]
    InvalidProviderFormat(String),

    #[error(
        "Invalid model format - expected 'backend/model' (e.g., 'anthropic/claude-sonnet-4.5' or 'openai/gpt-4o'), got '{0}'"
    )]
    InvalidModelFormat(String),

    #[error("Missing API key - set GITHUB_TOKEN or provide api_key in config")]
    MissingApiKey,

    #[error("Unknown GitHub Copilot backend: {0}")]
    UnknownBackend(String),

    #[error("Client creation failed: {0}")]
    ClientError(#[from] rig::http_client::Error),
}

#[cfg(test)]
#[path = "error_test.rs"]
mod error_test;
