use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::compaction::CompactionParams;

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    config: CompactionParams,
}

impl Session {
    /// Returns the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Creates a new session with the given ID.
    pub(crate) fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            config: CompactionParams::default(),
        }
    }

    /// Sets the session compaction configuration.
    pub fn set_compaction_config(&mut self, config: CompactionParams) {
        self.config = config;
    }

    /// Returns the session compaction configuration.
    pub fn compaction_config(&self) -> &CompactionParams {
        &self.config
    }

    /// Returns the session creation timestamp.
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Create a Session from stored metadata.
    pub(crate) fn from_metadata(metadata: SessionMetadata) -> Self {
        Self {
            id: metadata.session_id,
            created_at: metadata.created_at,
            config: CompactionParams::default(),
        }
    }
}

/// Metadata stored as the first line of a JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    #[serde(rename = "type")]
    pub metadata_type: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Extract a title from the first user message in a slice of messages.
///
/// Returns the first 80 characters of the first user message's text content,
/// stripped of leading/trailing whitespace and truncated at a word boundary.
/// Returns `None` if there is no user message with text content.
pub fn extract_title(messages: &[crate::types::Message]) -> Option<String> {
    let first_user_text = messages.iter().find_map(|msg| match msg {
        crate::types::Message::User { content } => {
            for item in content.iter() {
                if let crate::types::UserContent::Text(t) = item {
                    return Some(t.text.clone());
                }
            }
            None
        }
        _ => None,
    })?;

    let trimmed = first_user_text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Take first 80 chars, truncate at word boundary
    let max_len = 80usize;
    if trimmed.len() <= max_len {
        return Some(trimmed.to_string());
    }

    // Find last space within the first 80 chars
    let truncated = &trimmed[..max_len];
    if let Some(last_space) = truncated.rfind(' ') {
        Some(truncated[..last_space].to_string())
    } else {
        Some(truncated.to_string())
    }
}
