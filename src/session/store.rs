use super::SessionMetadata;
use chrono::{DateTime, Utc};
use rig::completion::Message;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionMarker {
    /// Discriminator — always "compaction_marker"
    #[serde(rename = "type")]
    pub entry_type: String,
    /// LLM-generated summary of older messages
    pub summary: String,
    /// Number of messages immediately before this marker that are "kept recent"
    pub kept_recent_count: usize,
    /// Number of messages that were summarized
    pub summarized_count: usize,
    /// Strategy used
    pub strategy: String,
    /// When compaction occurred
    pub created_at: DateTime<Utc>,
}

impl CompactionMarker {
    pub fn new(
        summary: String,
        kept_recent_count: usize,
        summarized_count: usize,
        strategy: &str,
    ) -> Self {
        Self {
            entry_type: "compaction_marker".to_string(),
            summary,
            kept_recent_count,
            summarized_count,
            strategy: strategy.to_string(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StoreEntry {
    Message(Message),
    Marker(CompactionMarker),
}

/// Trait for conversation storage implementations.
///
/// This trait defines the interface for storing and retrieving conversation messages.
/// Implementations must use static dispatch via generic bounds (T: ConversationStore),
/// NOT dynamic dispatch (Box<dyn ConversationStore>).
///
/// # Design principles
/// - Static dispatch: Use generics with trait bounds for zero-cost abstractions
/// - Error handling: All methods return Result for consistent error handling
/// - Session isolation: Each session is identified by a unique session_id string
pub trait ConversationStore {
    /// Load all messages for a session.
    ///
    /// Returns an empty vector for new or missing sessions (not an error).
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    ///
    /// # Returns
    /// A vector of messages in the order they were stored, or empty vec if session doesn't exist.
    fn load(&self, session_id: &str) -> Result<Vec<Message>, Box<dyn Error>>;

    /// Append new messages to an existing session.
    ///
    /// If the session doesn't exist, it will be created.
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    /// * `messages` - Slice of messages to append
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the operation fails.
    fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), Box<dyn Error>>;

    /// Clear all messages from a session.
    ///
    /// After this operation, the session will be deleted from storage.
    /// Does not return an error if the session doesn't exist.
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the operation fails.
    fn clear(&self, session_id: &str) -> Result<(), Box<dyn Error>>;

    /// Append a compaction marker to the session log.
    ///
    /// If the session doesn't exist, it will be created with a metadata line first.
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    /// * `marker` - The compaction marker to append
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the operation fails.
    fn append_marker(&self, session_id: &str, marker: &CompactionMarker)
        -> Result<(), Box<dyn Error>>;

    /// Load all entries (messages + markers) preserving JSONL order.
    ///
    /// Returns an empty vector for new or missing sessions (not an error).
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    ///
    /// # Returns
    /// A vector of StoreEntry in the order they were stored.
    fn load_all(&self, session_id: &str) -> Result<Vec<StoreEntry>, Box<dyn Error>>;
}

/// JSONL-based implementation of ConversationStore.
///
/// Stores each session in a separate .jsonl file with the following format:
/// - Line 1: SessionMetadata (JSON object)
/// - Line 2+: rig::completion::Message (JSON objects, one per line)
///
/// The rig::completion::Message type uses serde with:
/// `#[serde(tag = "role", rename_all = "lowercase")]`
/// which serializes as: `{"role":"user","content":[...]}`
#[derive(Debug, Clone)]
pub struct JsonlConversationStore {
    base_path: PathBuf,
}

impl JsonlConversationStore {
    /// Create a new JsonlConversationStore with the given base path.
    ///
    /// Session files will be stored as `base_path/session_id.jsonl`.
    ///
    /// # Arguments
    /// * `base_path` - Directory where session files will be stored
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get the file path for a session.
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_path.join(format!("{}.jsonl", session_id))
    }
}

impl ConversationStore for JsonlConversationStore {
    fn load(&self, session_id: &str) -> Result<Vec<Message>, Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Return empty vec if file doesn't exist (new session)
        if !path.exists() {
            return Ok(vec![]);
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Skip first line (metadata)
            if line_num == 0 {
                // Could validate metadata here, but we just skip it for now
                continue;
            }

            // Try to parse as Message
            match serde_json::from_str::<Message>(&line) {
                Ok(message) => messages.push(message),
                Err(e) => {
                    log::warn!(
                        "Skipping corrupt line {} in session {}: {}",
                        line_num + 1,
                        session_id,
                        e
                    );
                }
            }
        }

        Ok(messages)
    }

    fn append(&self, session_id: &str, messages: &[Message]) -> Result<(), Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Ensure base directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // If file doesn't exist, create it with metadata first
        if !path.exists() {
            let metadata = SessionMetadata {
                metadata_type: "session".to_string(),
                session_id: session_id.to_string(),
                created_at: chrono::Utc::now(),
                compaction_count: 0,
            };

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        writeln!(file, "{}", serde_json::to_string(&metadata)?)?;
    }

    // Append messages
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

        for message in messages {
            writeln!(file, "{}", serde_json::to_string(message)?)?;
        }

        Ok(())
    }

    fn clear(&self, session_id: &str) -> Result<(), Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Only attempt to remove if file exists
        if path.exists() {
            fs::remove_file(&path)?;
        }

        Ok(())
    }

    fn append_marker(
        &self,
        session_id: &str,
        marker: &CompactionMarker,
    ) -> Result<(), Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Ensure base directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // If file doesn't exist, create it with metadata first
        if !path.exists() {
            let metadata = SessionMetadata {
                metadata_type: "session".to_string(),
                session_id: session_id.to_string(),
                created_at: chrono::Utc::now(),
                compaction_count: 0,
            };

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;

            writeln!(file, "{}", serde_json::to_string(&metadata)?)?;
        }

        // Append marker
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        writeln!(file, "{}", serde_json::to_string(marker)?)?;

        Ok(())
    }

    fn load_all(&self, session_id: &str) -> Result<Vec<StoreEntry>, Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Return empty vec if file doesn't exist (new session)
        if !path.exists() {
            return Ok(vec![]);
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for (line_num, line) in reader.lines().enumerate() {
            let line = line?;

            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            // Skip first line (metadata)
            if line_num == 0 {
                continue;
            }

            // Parse as serde_json::Value first to discriminate type
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(value) => {
                    if value.get("type").and_then(|v| v.as_str()) == Some("compaction_marker") {
                        // Deserialize as CompactionMarker
                        match serde_json::from_value::<CompactionMarker>(value) {
                            Ok(marker) => entries.push(StoreEntry::Marker(marker)),
                            Err(e) => {
                                log::warn!(
                                    "Skipping corrupt marker at line {} in session {}: {}",
                                    line_num + 1,
                                    session_id,
                                    e
                                );
                            }
                        }
                    } else if value.get("role").is_some() {
                        // Deserialize as Message
                        match serde_json::from_value::<Message>(value) {
                            Ok(message) => entries.push(StoreEntry::Message(message)),
                            Err(e) => {
                                log::warn!(
                                    "Skipping corrupt message at line {} in session {}: {}",
                                    line_num + 1,
                                    session_id,
                                    e
                                );
                            }
                        }
                    } else {
                        log::warn!(
                            "Skipping unknown entry at line {} in session {}: no 'type' or 'role' key",
                            line_num + 1,
                            session_id,
                        );
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Skipping corrupt line {} in session {}: {}",
                        line_num + 1,
                        session_id,
                        e
                    );
                }
            }
        }

        Ok(entries)
    }
}

/// Extracts the LLM context from a sequence of store entries.
///
/// If there are compaction markers, uses the **last** marker to determine context:
/// - Prepends the marker's summary as a system message (if non-empty)
/// - Includes all messages after the marker (kept messages are re-appended after
///   the marker during compaction, followed by any post-compaction new messages)
///
/// If there are no markers, returns all messages.
pub fn extract_llm_context(entries: &[StoreEntry]) -> Vec<Message> {
    let last_marker_idx = entries
        .iter()
        .rposition(|e| matches!(e, StoreEntry::Marker(_)));

    match last_marker_idx {
        Some(idx) => {
            let marker = match &entries[idx] {
                StoreEntry::Marker(m) => m,
                _ => unreachable!(),
            };
            let mut context = Vec::new();
            if !marker.summary.is_empty() {
                context.push(Message::system(&marker.summary));
            }
            // All messages after marker (re-appended kept + any post-compaction new messages)
            for entry in &entries[idx + 1..] {
                if let StoreEntry::Message(m) = entry {
                    context.push(m.clone());
                }
            }
            context
        }
        None => entries
            .iter()
            .filter_map(|e| match e {
                StoreEntry::Message(m) => Some(m.clone()),
                _ => None,
            })
            .collect(),
    }
}
