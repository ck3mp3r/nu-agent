pub mod journal;
pub mod resolver;
mod store;

#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;

#[cfg(test)]
#[path = "journal_test.rs"]
mod journal_test;

pub use journal::JournalConversationMemory;
pub use store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::compaction::CompactionParams;

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

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    config: CompactionParams,
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
            config: CompactionParams::default(),
            compaction_count: 0,
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

    /// Returns the compaction count.
    pub fn compaction_count(&self) -> usize {
        self.compaction_count
    }

    /// Increments the compaction count by one.
    pub fn increment_compaction_count(&mut self) {
        self.compaction_count += 1;
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
    pub fn new_with_xdg_override(xdg_cache_home: Option<PathBuf>) -> Self {
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
            config: CompactionParams::default(), // Use default config for loaded sessions
            compaction_count: metadata.compaction_count,
        })
    }

    /// Lists all sessions in the cache directory with their metadata.
    pub fn list_sessions(&self) -> io::Result<Vec<SessionInfo>> {
        let mut sessions = Vec::new();

        let entries = match fs::read_dir(&self.cache_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(sessions);
            }
            Err(e) => return Err(e),
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            if let Ok(info) = self.extract_session_info(&path) {
                sessions.push(info);
            }
        }

        Ok(sessions)
    }

    fn extract_session_info(&self, path: &Path) -> io::Result<SessionInfo> {
        let file = fs::File::open(path)?;
        let mut reader = BufReader::new(file);

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

        let message_count = reader
            .lines()
            .map_while(Result::ok)
            .filter(|line| !line.trim().is_empty())
            .count();

        Ok(SessionInfo {
            id: metadata.session_id,
            message_count,
            compaction_count: metadata.compaction_count,
            last_active: metadata.created_at,
        })
    }

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

    pub fn resolve_cache_dir(xdg_cache_home: Option<PathBuf>) -> PathBuf {
        let base = xdg_cache_home
            .or_else(|| std::env::var("XDG_CACHE_HOME").ok().map(PathBuf::from))
            .or_else(|| crate::utils::xdg::cache_dir().ok())
            .expect("Failed to determine cache directory: XDG_CACHE_HOME not set and no platform default available");

        base.join("nu-agent").join("sessions")
    }

    fn ensure_directory_exists(path: &Path) -> io::Result<()> {
        if !path.exists() {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> io::Result<()> {
        let path = self.session_path(session_id);

        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Session '{}' not found", session_id),
            ));
        }

        fs::remove_file(&path)?;

        Ok(())
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod tool_session_test;
