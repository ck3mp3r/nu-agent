mod store;
#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;

#[cfg(test)]
#[path = "compaction_test.rs"]
mod compaction_test;

pub use store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::future::Future;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

/// SessionStore manages session storage using XDG Base Directory specification.
/// Sessions are stored in JSONL format in the cache directory.
///
/// Directory resolution follows XDG spec:
/// 1. If XDG_CACHE_HOME is set, use $XDG_CACHE_HOME/nu-agent/sessions
/// 2. Otherwise, use ~/.cache/nu-agent/sessions (or platform equivalent)
///
/// Reference: <https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html>
#[derive(Debug, Clone)]
pub struct SessionStore {
    cache_dir: PathBuf,
}

/// Strategy for compacting messages when threshold is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    /// Summarize old messages and keep a recent verbatim window.
    #[serde(
        rename = "sliding_summary",
        alias = "truncate",
        alias = "sliding",
        alias = "summarize"
    )]
    SlidingSummary,
    /// Drop old messages, keep only the last N. No summarization.
    #[serde(rename = "sliding_window")]
    SlidingWindow,
    /// Keep newest messages that fit within a token budget. No summarization.
    #[serde(rename = "token_truncate")]
    TokenTruncate,
}

impl CompactionStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlidingSummary => "sliding_summary",
            Self::SlidingWindow => "sliding_window",
            Self::TokenTruncate => "token_truncate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOutcome {
    pub summarized_count: usize,
    pub kept_recent_count: usize,
    pub summary_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionInvocationMode {
    Threshold,
    Force,
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
    /// Maximum token budget for TokenTruncate strategy.
    pub token_budget: Option<usize>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            compaction_threshold: 100, // Default threshold
            compaction_strategy: CompactionStrategy::SlidingSummary, // Canonical strategy
            keep_recent: 10,           // Default keep last 10 messages
            token_budget: None,        // No token budget by default
        }
    }
}

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
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

    /// Creates a new session with the given ID.
    fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
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

    /// Compacts messages using rig memory and ConversationStore.
    ///
    /// This method:
    /// 1. Loads messages from InMemoryConversationMemory
    /// 2. Splits at `len - keep_recent`
    /// 3. Formats old messages for summarization
    /// 4. Calls summarizer with old messages
    /// 5. Builds compacted list: [Message::system(summary)] + recent
    /// 6. Updates compaction_count
    /// 7. Appends compaction marker to ConversationStore (durable commit point)
    /// 8. Clears memory and appends compacted messages (with rollback on failure)
    ///
    /// # Arguments
    /// * `memory` - InMemoryConversationMemory containing session messages
    /// * `store` - ConversationStore for persistent JSONL storage
    /// * `summarizer` - Function that takes rig messages and returns a summary string
    ///
    /// # Returns
    /// CompactionOutcome with counts and summary text
    ///
    /// # Errors
    /// Returns an error if memory operations, summarizer, or store operations fail.
    pub async fn compact<F, Fut, S>(
        &mut self,
        memory: &rig::memory::InMemoryConversationMemory,
        store: &S,
        summarizer: F,
    ) -> io::Result<CompactionOutcome>
    where
        F: FnOnce(&[rig::completion::Message]) -> Fut,
        Fut: Future<Output = io::Result<String>>,
        S: ConversationStore,
    {
        use rig::memory::ConversationMemory;

        let keep_count = self.config.keep_recent;

        // Load messages from memory
        let messages = memory
            .load(&self.id)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Strategy-specific: build compacted messages, summary text, and counts.
        // TokenTruncate uses token budgets on ALL messages (ignores keep_recent/split).
        // SlidingSummary and SlidingWindow split messages into old/recent at a fixed index.
        let (llm_context, summary_text, summarized_count, kept_recent_count, strategy_name) =
            match self.config.compaction_strategy {
                CompactionStrategy::TokenTruncate => {
                    let budget = self
                        .config
                        .token_budget
                        .unwrap_or(self.config.compaction_threshold * 100);
                    let mut kept: Vec<rig::completion::Message> = Vec::new();
                    let mut total_tokens: usize = 0;
                    for msg in messages.iter().rev() {
                        let msg_tokens = estimate_tokens(msg);
                        if total_tokens + msg_tokens > budget && !kept.is_empty() {
                            break;
                        }
                        total_tokens += msg_tokens;
                        kept.push(msg.clone());
                    }
                    kept.reverse();
                    if let Some(rig::completion::Message::System { .. }) = messages.first()
                        && !matches!(
                            kept.first(),
                            Some(rig::completion::Message::System { .. })
                        )
                    {
                        kept.insert(0, messages[0].clone());
                    }
                    let kept_count = kept.len();
                    let dropped = messages.len().saturating_sub(kept_count);
                    (kept, String::new(), dropped, kept_count, "token_truncate")
                }
                _ => {
                    // For SlidingSummary and SlidingWindow, use keep_recent split
                    if messages.len() <= keep_count {
                        return Ok(CompactionOutcome {
                            summarized_count: 0,
                            kept_recent_count: messages.len(),
                            summary_text: String::new(),
                        });
                    }

                    // Split messages into old (to summarize) and recent (to keep).
                    // Use group-aware split to avoid breaking tool call/result pairs.
                    let naive_index = messages.len() - keep_count;
                    let split_index = find_safe_split_index(&messages, naive_index);
                    let old_messages = &messages[..split_index];
                    let recent_messages = &messages[split_index..];
                    let summarized_count = old_messages.len();
                    let kept_recent_count = recent_messages.len();

                    match self.config.compaction_strategy {
                        CompactionStrategy::SlidingSummary => {
                            let summary = summarizer(old_messages).await?;
                            let summary_message = rig::completion::Message::system(&summary);
                            let mut compacted = vec![summary_message];
                            compacted.extend_from_slice(recent_messages);
                            (compacted, summary, summarized_count, kept_recent_count, "sliding_summary")
                        }
                        CompactionStrategy::SlidingWindow => (
                            recent_messages.to_vec(),
                            String::new(),
                            summarized_count,
                            kept_recent_count,
                            "sliding_window",
                        ),
                        CompactionStrategy::TokenTruncate => {
                            unreachable!("TokenTruncate handled above")
                        }
                    }
                }
            };

        // Increment compaction count
        self.compaction_count += 1;

        // Append compaction marker to store
        let marker = CompactionMarker::new(
            summary_text.clone(),
            kept_recent_count,
            summarized_count,
            strategy_name,
        );
        store
            .append_marker(&self.id, &marker)
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Now update in-memory state
        memory
            .clear(&self.id)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;

        // Rollback: if append fails after clear, reload LLM context from store
        if let Err(e) = memory.append(&self.id, llm_context).await {
            if let Ok(entries) = store.load_all(&self.id) {
                let context = extract_llm_context(&entries);
                let _ = memory.append(&self.id, context).await;
            }
            return Err(io::Error::other(e.to_string()));
        }

        Ok(CompactionOutcome {
            summarized_count,
            kept_recent_count,
            summary_text,
        })
    }
}

/// Metadata stored as the first line of a JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    #[serde(rename = "type")]
    pub metadata_type: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub compaction_count: usize,
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
    /// The first line contains metadata. This method only loads metadata,
    /// not the message history. To load messages, use ConversationStore.
    ///
    /// # Arguments
    /// * `session_id` - The ID of the session to load
    ///
    /// # Returns
    /// A Session with its metadata loaded from the JSONL file.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file cannot be read
    /// - The file is empty (no metadata line)
    /// - The metadata line cannot be parsed as JSON
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

        Ok(Session {
            id: metadata.session_id,
            created_at: metadata.created_at,
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
            .or_else(|| crate::utils::xdg::cache_dir().ok())
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

/// Returns true if the message is an Assistant message containing at least one ToolCall.
fn has_tool_call(msg: &rig::completion::Message) -> bool {
    match msg {
        rig::completion::Message::Assistant { content, .. } => {
            content
                .iter()
                .any(|c| matches!(c, rig::completion::message::AssistantContent::ToolCall(_)))
        }
        _ => false,
    }
}

/// Returns true if the message is a User message containing at least one ToolResult.
fn has_tool_result(msg: &rig::completion::Message) -> bool {
    match msg {
        rig::completion::Message::User { content } => {
            content
                .iter()
                .any(|c| matches!(c, rig::completion::message::UserContent::ToolResult(_)))
        }
        _ => false,
    }
}

/// Adjusts a target split index so that it never falls between a ToolCall and its
/// corresponding ToolResult. If the boundary would separate a pair, it moves backward
/// until the boundary is safe.
fn find_safe_split_index(messages: &[rig::completion::Message], target_index: usize) -> usize {
    if target_index >= messages.len() {
        return messages.len();
    }
    if target_index == 0 {
        return 0;
    }
    let mut idx = target_index;
    loop {
        if idx == 0 {
            break;
        }
        if has_tool_result(&messages[idx]) || has_tool_call(&messages[idx - 1]) {
            idx -= 1;
        } else {
            break;
        }
    }
    idx
}

/// Estimates the token count for a message using a simple chars/4 heuristic.
fn estimate_tokens(msg: &rig::completion::Message) -> usize {
    let text = serde_json::to_string(msg).unwrap_or_default();
    text.len() / 4
}

#[cfg(test)]
mod test;
