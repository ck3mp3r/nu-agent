use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// SessionStore manages session storage using XDG Base Directory specification.
/// Sessions are stored in JSONL format in the cache directory.
///
/// Directory resolution follows XDG spec:
/// 1. If XDG_CACHE_HOME is set, use $XDG_CACHE_HOME/nu-agent/sessions
/// 2. Otherwise, use ~/.cache/nu-agent/sessions (or platform equivalent)
///
/// Reference: https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html
#[derive(Debug, Clone)]
pub struct SessionStore {
    cache_dir: PathBuf,
}

/// Strategy for compacting messages when threshold is exceeded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Summarize messages using LLM (tasks 1.11)
    Summarize,
    /// Truncate oldest messages (task 1.12)
    Truncate,
    /// Keep sliding window of recent messages (task 1.13)
    Sliding,
}

/// Configuration for session behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum number of messages before compaction is triggered.
    pub compaction_threshold: usize,
    /// Strategy to use for compaction.
    pub compaction_strategy: CompactionStrategy,
    /// Number of recent messages to keep during truncation compaction.
    pub keep_recent: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            compaction_threshold: 100,                         // Default threshold
            compaction_strategy: CompactionStrategy::Truncate, // Default strategy
            keep_recent: 10,                                   // Default keep last 10 messages
        }
    }
}

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
    messages: Vec<Message>,
    #[serde(default)]
    config: SessionConfig,
    #[serde(default)]
    compaction_count: usize,
}

/// Information about a session, extracted from metadata without loading all messages.
/// Used for listing sessions efficiently.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Session identifier
    pub id: String,
    /// Number of messages in the session (excluding metadata line)
    pub message_count: usize,
    /// Number of compactions performed on this session
    pub compaction_count: usize,
    /// Timestamp of last activity (currently same as created_at)
    pub last_active: DateTime<Utc>,
}

impl Session {
    /// Returns the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns a reference to the messages in this session.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Creates a new session with the given ID.
    fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            messages: Vec::new(),
            config: SessionConfig::default(),
            compaction_count: 0,
        }
    }

    /// Sets the session configuration.
    pub fn set_config(&mut self, config: SessionConfig) {
        self.config = config;
    }

    /// Returns the session configuration.
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Returns the session creation timestamp.
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Returns the compaction count.
    pub fn compaction_count(&self) -> usize {
        self.compaction_count
    }

    /// Adds a message to the session.
    ///
    /// This method appends the message to the session's messages vector and
    /// persists it to the JSONL file. If the number of messages exceeds the
    /// compaction threshold, compaction will be triggered (placeholder for now).
    ///
    /// # Arguments
    /// * `store` - The SessionStore used to resolve the file path
    /// * `message` - The message to add
    ///
    /// # Returns
    /// Ok(()) if the message was successfully added, Err otherwise.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The message cannot be serialized to JSON
    /// - The file cannot be opened or written to
    pub fn add_message(&mut self, store: &SessionStore, message: Message) -> io::Result<()> {
        // Append message to the JSONL file
        self.append_message(store, message.clone())?;

        // Add to in-memory vector
        self.messages.push(message);

        // Check if compaction threshold is exceeded
        if self.messages.len() > self.config.compaction_threshold {
            // Placeholder: trigger compaction
            // TODO: Implement actual compaction in future tasks
            self.trigger_compaction_placeholder();
        }

        Ok(())
    }

    /// Checks if compaction is needed and performs it using the configured strategy.
    ///
    /// Compaction is triggered when the number of messages exceeds the configured
    /// `compaction_threshold`. The specific compaction strategy is determined by
    /// `config.compaction_strategy`.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used to resolve file paths
    ///
    /// # Returns
    /// Ok(true) if compaction was triggered and performed, Ok(false) if no compaction
    /// was needed, or Err if compaction failed.
    ///
    /// # Errors
    /// Returns an error if the chosen compaction strategy fails.
    pub fn maybe_compact(&mut self, store: &SessionStore) -> io::Result<bool> {
        // Check if we exceed the threshold
        if self.messages.len() <= self.config.compaction_threshold {
            return Ok(false);
        }

        // Trigger compaction based on strategy
        match self.config.compaction_strategy {
            CompactionStrategy::Summarize => self.compact_summarize(store),
            CompactionStrategy::Truncate => self.compact_truncate(store),
            CompactionStrategy::Sliding => self.compact_sliding(store),
        }?;

        Ok(true)
    }

    /// Compacts messages using summarization strategy with a custom summarizer function.
    ///
    /// Splits messages into "old" (to be summarized) and "recent" (to keep at full fidelity).
    /// The summarizer function is called with old messages and returns a summary string.
    /// The summary replaces all old messages as a single "system" role message.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used for file operations
    /// * `summarizer` - Function that takes messages and returns a summary string
    ///
    /// # Returns
    /// Ok(()) when summarization succeeds.
    ///
    /// # Errors
    /// Returns an error if the summarizer fails or file operations fail.
    pub fn compact_summarize_with<F>(
        &mut self,
        store: &SessionStore,
        summarizer: F,
    ) -> io::Result<()>
    where
        F: FnOnce(&[Message]) -> io::Result<String>,
    {
        let keep_count = self.config.keep_recent;

        // If we have fewer messages than keep_recent, nothing to do
        if self.messages.len() <= keep_count {
            return Ok(());
        }

        // Split messages into old (to summarize) and recent (to keep)
        let split_index = self.messages.len() - keep_count;
        let old_messages = &self.messages[..split_index];
        let recent_messages = &self.messages[split_index..];

        // Call summarizer with old messages
        let summary = summarizer(old_messages)?;

        // Create summary message with "system" role
        let summary_message = Message::new("system".to_string(), summary);

        // Replace messages: [summary] + recent messages
        let mut new_messages = vec![summary_message];
        new_messages.extend_from_slice(recent_messages);
        self.messages = new_messages;

        // Increment compaction count
        self.compaction_count += 1;

        // Rewrite the JSONL file with updated metadata and compacted messages
        self.rewrite_jsonl(store)?;

        Ok(())
    }

    /// Compacts messages using summarization strategy.
    ///
    /// This is a stub that will be implemented once we have LLM integration in the Session.
    /// For now, it just returns Ok(()).
    ///
    /// # Arguments
    /// * `store` - The SessionStore used for file operations
    ///
    /// # Returns
    /// Ok(()) when summarization succeeds.
    fn compact_summarize(&mut self, _store: &SessionStore) -> io::Result<()> {
        // Stub: Full LLM integration will be implemented in future tasks
        // For now, tests use compact_summarize_with() directly with mock summarizers
        Ok(())
    }

    /// Compacts messages using truncation strategy.
    ///
    /// Keeps only the last N messages (configured via `keep_recent`),
    /// dropping all older messages. After truncation, rewrites the JSONL
    /// file with the new message list and increments the compaction count.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used for file operations
    ///
    /// # Returns
    /// Ok(()) when truncation succeeds.
    ///
    /// # Errors
    /// Returns an error if file operations fail.
    fn compact_truncate(&mut self, store: &SessionStore) -> io::Result<()> {
        let keep_count = self.config.keep_recent;

        // If we have fewer messages than keep_recent, nothing to do
        if self.messages.len() <= keep_count {
            return Ok(());
        }

        // Keep only the last N messages
        let start_index = self.messages.len() - keep_count;
        self.messages = self.messages[start_index..].to_vec();

        // Increment compaction count
        self.compaction_count += 1;

        // Rewrite the JSONL file with updated metadata and truncated messages
        self.rewrite_jsonl(store)?;

        Ok(())
    }

    /// Rewrites the entire JSONL file with current metadata and messages.
    ///
    /// This is used after compaction to persist the new message list.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used to resolve the file path
    ///
    /// # Returns
    /// Ok(()) if the file was successfully rewritten.
    ///
    /// # Errors
    /// Returns an error if file operations or JSON serialization fail.
    fn rewrite_jsonl(&self, store: &SessionStore) -> io::Result<()> {
        let path = store.session_path(&self.id);

        let metadata = SessionMetadata {
            metadata_type: "meta".to_string(),
            session_id: self.id.clone(),
            created_at: self.created_at,
            compaction_count: self.compaction_count,
        };

        let metadata_json = serde_json::to_string(&metadata).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to serialize metadata: {}", e),
            )
        })?;

        let mut content = metadata_json;
        content.push('\n');

        // Append all messages
        for message in &self.messages {
            let message_json = serde_json::to_string(message).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to serialize message: {}", e),
                )
            })?;
            content.push_str(&message_json);
            content.push('\n');
        }

        // Atomic write pattern: write to temp file in same directory, then rename
        // This ensures crash-safety - if we crash during write, original file is intact
        let parent_dir = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid path"))?;

        // Create temp file in the same directory as target (required for atomic rename)
        let mut temp_file = NamedTempFile::new_in(parent_dir)?;

        // Write all content to temp file
        temp_file.write_all(content.as_bytes())?;

        // Sync to disk before rename to ensure data is persisted
        temp_file.flush()?;
        temp_file.as_file().sync_all()?;

        // Atomic rename: this is crash-safe - either succeeds completely or fails completely
        // If we crash here, temp file exists but original is intact
        // If rename succeeds, temp file becomes the new file atomically
        temp_file.persist(&path)?;

        Ok(())
    }

    /// Compacts messages using sliding window strategy.
    ///
    /// Keeps only the last N messages (configured via `keep_recent`),
    /// dropping all older messages. This is similar to truncation, but the name
    /// emphasizes the "sliding window" behavior where new messages push out old ones.
    ///
    /// After compaction, rewrites the JSONL file with the new message list
    /// and increments the compaction count.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used for file operations
    ///
    /// # Returns
    /// Ok(()) when sliding window compaction succeeds.
    ///
    /// # Errors
    /// Returns an error if file operations fail.
    fn compact_sliding(&mut self, store: &SessionStore) -> io::Result<()> {
        let keep_count = self.config.keep_recent;

        // If we have fewer messages than keep_recent, nothing to do
        if self.messages.len() <= keep_count {
            return Ok(());
        }

        // Keep only the last N messages
        let start_index = self.messages.len() - keep_count;
        self.messages = self.messages[start_index..].to_vec();

        // Increment compaction count
        self.compaction_count += 1;

        // Rewrite the JSONL file with updated metadata and compacted messages
        self.rewrite_jsonl(store)?;

        Ok(())
    }

    /// Placeholder for compaction trigger.
    /// This will be implemented in future tasks.
    fn trigger_compaction_placeholder(&self) {
        // Placeholder: no-op for now
        // Future implementation will handle message compaction
    }

    /// Appends a message to the session's JSONL file.
    ///
    /// The message is serialized as JSON and appended as a new line to the file.
    /// The metadata line (first line) is not modified.
    ///
    /// # Arguments
    /// * `store` - The SessionStore used to resolve the file path
    /// * `message` - The message to append
    ///
    /// # Returns
    /// Ok(()) if the message was successfully appended, Err otherwise.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The message cannot be serialized to JSON
    /// - The file cannot be opened or written to
    pub fn append_message(&mut self, store: &SessionStore, message: Message) -> io::Result<()> {
        let path = store.session_path(&self.id);

        // Serialize message to JSON
        let message_json = serde_json::to_string(&message).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to serialize message: {}", e),
            )
        })?;

        // Open file in append mode and write the message line
        let mut file = OpenOptions::new().append(true).open(&path)?;

        writeln!(file, "{}", message_json)?;

        Ok(())
    }
}

/// Metadata stored as the first line of a JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMetadata {
    #[serde(rename = "type")]
    metadata_type: String,
    session_id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    compaction_count: usize,
}

/// Represents a message in a session.
/// Messages are appended to the JSONL file after the metadata line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    role: String,
    content: String,
    timestamp: DateTime<Utc>,
}

impl Message {
    /// Creates a new message with the given role and content.
    /// The timestamp is automatically set to the current time.
    pub fn new(role: String, content: String) -> Self {
        Self {
            role,
            content,
            timestamp: Utc::now(),
        }
    }

    /// Returns the message role.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the message content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Returns the message timestamp.
    pub fn timestamp(&self) -> &DateTime<Utc> {
        &self.timestamp
    }
}

impl SessionStore {
    /// Creates a new SessionStore with default XDG cache directory.
    ///
    /// Resolves the cache directory according to XDG Base Directory specification:
    /// - Uses $XDG_CACHE_HOME/nu-agent/sessions if XDG_CACHE_HOME is set
    /// - Otherwise uses ~/.cache/nu-agent/sessions (or platform equivalent from dirs crate)
    ///
    /// Creates the directory if it doesn't exist.
    ///
    /// # Panics
    /// Panics if the cache directory cannot be determined or created.
    pub fn new() -> Self {
        let xdg_override = std::env::var("XDG_CACHE_HOME").ok().map(PathBuf::from);
        Self::new_with_xdg_override(xdg_override)
    }

    /// Creates a new SessionStore with a custom cache directory.
    /// Used for testing and when you need explicit control over the storage location.
    ///
    /// Creates the directory if it doesn't exist.
    ///
    /// # Panics
    /// Panics if the directory cannot be created.
    pub fn new_with_cache_dir(cache_dir: PathBuf) -> Self {
        Self::ensure_directory_exists(&cache_dir).expect("Failed to create cache directory");

        Self { cache_dir }
    }

    /// Creates a new SessionStore with optional XDG_CACHE_HOME override.
    /// Used internally and for testing.
    ///
    /// # Arguments
    /// * `xdg_cache_home` - Optional XDG_CACHE_HOME path. If None, uses platform default.
    ///
    /// # Panics
    /// Panics if the cache directory cannot be determined or created.
    pub(crate) fn new_with_xdg_override(xdg_cache_home: Option<PathBuf>) -> Self {
        let cache_dir = Self::resolve_cache_dir(xdg_cache_home);
        Self::new_with_cache_dir(cache_dir)
    }

    /// Returns the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Gets an existing session or creates a new one.
    ///
    /// If `id` is None, auto-generates a session ID with format `session-<timestamp>`.
    /// If the session file exists, loads it from JSONL. Otherwise, creates a new session
    /// and writes it to a JSONL file.
    ///
    /// # Arguments
    /// * `id` - Optional session ID. If None, generates `session-YYYYMMDD-HHMMSS`.
    ///
    /// # Returns
    /// A Session instance, either loaded or newly created.
    ///
    /// # Errors
    /// Returns an error if file operations fail or JSONL parsing fails.
    pub fn get_or_create(&self, id: Option<String>) -> io::Result<Session> {
        let session_id = id.unwrap_or_else(|| self.generate_session_id());
        let session_path = self.session_path(&session_id);

        if session_path.exists() {
            self.load_session(&session_id)
        } else {
            let session = Session::new(session_id);
            self.save_session(&session)?;
            Ok(session)
        }
    }

    /// Generates a unique session ID with format: session-YYYYMMDD-HHMMSS-micros
    fn generate_session_id(&self) -> String {
        let now = Utc::now();
        format!(
            "session-{}-{}",
            now.format("%Y%m%d-%H%M%S"),
            now.timestamp_subsec_micros()
        )
    }

    /// Returns the path to a session's JSONL file.
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.jsonl", session_id))
    }

    /// Loads a session from its JSONL file.
    ///
    /// The first line contains metadata, subsequent lines contain messages.
    ///
    /// # Arguments
    /// * `session_id` - The ID of the session to load
    ///
    /// # Returns
    /// A Session with its metadata and messages loaded from the JSONL file.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file cannot be read
    /// - The file is empty (no metadata line)
    /// - The metadata line cannot be parsed as JSON
    /// - Any message line cannot be parsed as JSON
    pub fn load_session(&self, session_id: &str) -> io::Result<Session> {
        let path = self.session_path(session_id);
        let content = fs::read_to_string(&path)?;

        let mut lines = content.lines();
        let metadata_line = lines
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty JSONL file"))?;

        let metadata: SessionMetadata = serde_json::from_str(metadata_line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse metadata: {}", e),
            )
        })?;

        // Parse all remaining lines as messages
        let mut messages = Vec::new();
        for (line_num, line) in lines.enumerate() {
            // Skip empty lines
            if line.trim().is_empty() {
                continue;
            }

            let message: Message = serde_json::from_str(line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to parse message on line {}: {}", line_num + 2, e),
                )
            })?;

            messages.push(message);
        }

        Ok(Session {
            id: metadata.session_id,
            created_at: metadata.created_at,
            messages,
            config: SessionConfig::default(), // Use default config for loaded sessions
            compaction_count: metadata.compaction_count,
        })
    }

    /// Lists all sessions in the cache directory with their metadata.
    ///
    /// Reads all .jsonl files in the cache directory and extracts metadata
    /// from the first line of each file. Does not load full message content.
    ///
    /// # Returns
    /// A vector of SessionInfo containing session metadata and statistics.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The cache directory cannot be read
    /// - Any session file cannot be read
    /// - Any metadata line cannot be parsed as JSON
    pub fn list_sessions(&self) -> io::Result<Vec<SessionInfo>> {
        let mut sessions = Vec::new();

        // Read all entries in cache directory
        let entries = match fs::read_dir(&self.cache_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Directory doesn't exist yet, return empty list
                return Ok(sessions);
            }
            Err(e) => return Err(e),
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Only process .jsonl files
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            // Extract session info from this file
            if let Ok(info) = self.extract_session_info(&path) {
                sessions.push(info);
            }
        }

        Ok(sessions)
    }

    /// Extracts session info from a JSONL file by reading only the metadata line
    /// and counting message lines.
    ///
    /// # Arguments
    /// * `path` - Path to the session JSONL file
    ///
    /// # Returns
    /// SessionInfo with extracted metadata and statistics.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The file is empty (no metadata line)
    /// - The metadata line cannot be parsed as JSON
    fn extract_session_info(&self, path: &Path) -> io::Result<SessionInfo> {
        let file = fs::File::open(path)?;
        let mut reader = BufReader::new(file);

        // Read first line (metadata)
        let mut metadata_line = String::new();
        reader.read_line(&mut metadata_line)?;

        if metadata_line.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Empty JSONL file",
            ));
        }

        let metadata: SessionMetadata = serde_json::from_str(&metadata_line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse metadata: {}", e),
            )
        })?;

        // Count remaining non-empty lines (messages)
        let message_count = reader
            .lines()
            .map_while(Result::ok)
            .filter(|line| !line.trim().is_empty())
            .count();

        Ok(SessionInfo {
            id: metadata.session_id,
            message_count,
            compaction_count: metadata.compaction_count,
            last_active: metadata.created_at, // For now, use created_at as last_active
        })
    }

    /// Saves a session to its JSONL file.
    ///
    /// Creates the file with metadata as the first line.
    fn save_session(&self, session: &Session) -> io::Result<()> {
        let path = self.session_path(&session.id);

        let metadata = SessionMetadata {
            metadata_type: "meta".to_string(),
            session_id: session.id.clone(),
            created_at: session.created_at,
            compaction_count: session.compaction_count,
        };

        let metadata_json = serde_json::to_string(&metadata).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to serialize metadata: {}", e),
            )
        })?;

        let mut content = metadata_json;
        content.push('\n');

        fs::write(&path, content)?;
        Ok(())
    }

    /// Resolves the cache directory according to XDG Base Directory specification.
    ///
    /// # Arguments
    /// * `xdg_cache_home` - Optional XDG_CACHE_HOME override. If None, uses env var or default.
    ///
    /// # Returns
    /// PathBuf pointing to the resolved cache directory (not created yet).
    ///
    /// # Panics
    /// Panics if the cache directory cannot be determined (e.g., HOME not set on Unix).
    pub(crate) fn resolve_cache_dir(xdg_cache_home: Option<PathBuf>) -> PathBuf {
        let base = xdg_cache_home
            .or_else(|| std::env::var("XDG_CACHE_HOME").ok().map(PathBuf::from))
            .or_else(dirs::cache_dir)
            .expect("Failed to determine cache directory: XDG_CACHE_HOME not set and no platform default available");

        base.join("nu-agent").join("sessions")
    }

    /// Ensures the directory exists, creating it if necessary.
    ///
    /// # Arguments
    /// * `path` - Path to the directory to create
    ///
    /// # Returns
    /// Ok(()) if directory exists or was created successfully, Err otherwise.
    fn ensure_directory_exists(path: &Path) -> io::Result<()> {
        if !path.exists() {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    /// Deletes a session by removing its JSONL file from the cache directory.
    ///
    /// # Arguments
    /// * `session_id` - The ID of the session to delete
    ///
    /// # Returns
    /// Ok(()) if the session was successfully deleted.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The session file doesn't exist (io::ErrorKind::NotFound)
    /// - The file cannot be deleted due to permissions or other I/O errors
    pub fn delete_session(&self, session_id: &str) -> io::Result<()> {
        let path = self.session_path(session_id);

        // Check if the file exists before attempting to delete
        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Session '{}' not found", session_id),
            ));
        }

        // Delete the file
        fs::remove_file(&path)?;

        Ok(())
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
