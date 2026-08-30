use chrono::{DateTime, Utc};

/// Information about a session, extracted from metadata without loading all messages.
/// Used for listing sessions efficiently.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Session identifier
    pub id: String,
    /// Number of messages in the session (excluding metadata line)
    pub message_count: usize,
    /// Timestamp of last activity (currently same as created_at)
    pub last_active: DateTime<Utc>,
    /// Optional title extracted from the first user message
    pub title: Option<String>,
}
