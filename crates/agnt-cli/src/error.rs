//! Error type for the `agent` CLI.

// region:    --- Types

pub type Result<T> = core::result::Result<T, Error>;

/// Errors surfaced by the CLI runtime modes.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A CLI-local failure (terminal setup, renderer construction).
    #[error("{0}")]
    Custom(String),

    /// An error reported by the agent runtime.
    #[error(transparent)]
    Runtime(#[from] nu_protocol::LabeledError),
}

// endregion: --- Types
