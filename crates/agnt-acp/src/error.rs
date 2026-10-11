use thiserror::Error;

/// Unified error type for all ACP operations.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum AcpError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("session already exists: {0}")]
    SessionAlreadyExists(String),

    #[error("invalid session state: {0}")]
    InvalidSessionState(String),

    #[error("runtime error: {0}")]
    Runtime(String),
}

impl From<agent_client_protocol::Error> for AcpError {
    fn from(err: agent_client_protocol::Error) -> Self {
        AcpError::Runtime(err.to_string())
    }
}

pub type Result<T> = core::result::Result<T, AcpError>;
