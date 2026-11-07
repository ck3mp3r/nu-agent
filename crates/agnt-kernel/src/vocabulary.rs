//! Vocabulary the record stream and its consumers share.
//!
//! Identity only. A tool's name is data every layer needs; what a layer draws
//! from a record is that layer's own business.

// region:    --- Types

/// A tool's name. Newtype so tool identity stays typed wherever it travels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolName(pub String);

// endregion: --- Types

// region:    --- Froms

impl From<&str> for ToolName {
    fn from(name: &str) -> Self {
        Self(name.to_string())
    }
}

impl From<String> for ToolName {
    fn from(name: String) -> Self {
        Self(name)
    }
}

// endregion: --- Froms
