use super::SessionMetadata;
use rig::completion::Message;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

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

    /// Atomically rewrite the entire session with new metadata and messages.
    ///
    /// This operation should be atomic (write to temporary file, then rename).
    /// Replaces all existing content for this session.
    ///
    /// # Arguments
    /// * `session_id` - The unique identifier for the session
    /// * `metadata` - Session metadata to write as the first line
    /// * `messages` - Complete list of messages to store
    ///
    /// # Returns
    /// Ok(()) on success, or an error if the operation fails.
    fn rewrite(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
        messages: &[Message],
    ) -> Result<(), Box<dyn Error>>;

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
                    eprintln!(
                        "Warning: Skipping corrupt line {} in session {}: {}",
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

    fn rewrite(
        &self,
        session_id: &str,
        metadata: &SessionMetadata,
        messages: &[Message],
    ) -> Result<(), Box<dyn Error>> {
        let path = self.session_path(session_id);

        // Ensure base directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Use atomic write: temp file + rename
        let mut temp_file =
            NamedTempFile::new_in(path.parent().unwrap_or_else(|| std::path::Path::new(".")))?;

        // Write metadata as first line
        writeln!(temp_file, "{}", serde_json::to_string(metadata)?)?;

        // Write all messages
        for message in messages {
            writeln!(temp_file, "{}", serde_json::to_string(message)?)?;
        }

        // Atomic rename
        temp_file.persist(&path)?;

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
}
