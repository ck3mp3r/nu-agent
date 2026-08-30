use super::{SessionInfo, SessionMetadata, extract_title};
use crate::types::Message;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionMarker {
    /// Discriminator — always "compaction_marker"
    #[serde(rename = "type")]
    pub entry_type: String,
    /// LLM-generated summary of older messages
    pub summary: String,
    /// When compaction occurred
    #[serde(default)]
    pub created_at: DateTime<Utc>,
}

impl CompactionMarker {
    pub fn new(summary: String, created_at: DateTime<Utc>) -> Self {
        Self {
            entry_type: "compaction_marker".to_string(),
            summary,
            created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub enum StoreEntry {
    Message(Message),
    Marker(CompactionMarker),
}

pub trait SessionStore {
    type Error: std::error::Error + Send + Sync;

    /// Persist a new session with its first messages. Metadata derived atomically.
    fn create(
        &self,
        id: &str,
        first_messages: &[Message],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Load full session: metadata + all entries. None if doesn't exist or empty.
    fn load(
        &self,
        id: &str,
    ) -> impl std::future::Future<
        Output = Result<Option<(SessionMetadata, Vec<StoreEntry>)>, Self::Error>,
    > + Send;

    /// Append entries (messages or compaction markers) to existing session.
    fn append(
        &self,
        id: &str,
        entries: &[StoreEntry],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Replace all entries for a session (compaction). Keeps metadata.
    fn replace_entries(
        &self,
        id: &str,
        entries: &[StoreEntry],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;

    /// List all persisted sessions (metadata only, newest first). Filters empty sessions.
    fn list(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<SessionInfo>, Self::Error>> + Send;

    /// Delete a session entirely.
    fn delete(&self, id: &str)
    -> impl std::future::Future<Output = Result<(), Self::Error>> + Send;
}

/// JSONL-based implementation of SessionStore.
///
/// Stores each session in a separate .jsonl file with the following format:
/// - Line 1: SessionMetadata (JSON object)
/// - Line 2+: StoreEntry items (JSON objects, one per line)
///
/// Messages use serde with role-tagged serialization:
/// `{"role":"user","content":[...]}`
/// Markers use type-tagged serialization:
/// `{"type":"compaction_marker",...}`
#[derive(Debug, Clone)]
pub struct FsSessionStore {
    base_path: PathBuf,
}

impl FsSessionStore {
    /// Create a new FsSessionStore with the given base path.
    ///
    /// Session files will be stored as `base_path/session_id.jsonl`.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get the file path for a session.
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_path.join(format!("{session_id}.jsonl"))
    }

    /// Parse a single JSONL line (after metadata) into a StoreEntry.
    fn parse_entry(line: &str) -> Option<StoreEntry> {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;

        if value.get("type").and_then(|v| v.as_str()) == Some("compaction_marker") {
            serde_json::from_value::<CompactionMarker>(value)
                .ok()
                .map(StoreEntry::Marker)
        } else if value.get("role").is_some() {
            serde_json::from_value::<Message>(value)
                .ok()
                .map(StoreEntry::Message)
        } else {
            None
        }
    }

    /// Serialize a StoreEntry to a JSON string.
    fn serialize_entry(entry: &StoreEntry) -> io::Result<String> {
        let value = match entry {
            StoreEntry::Message(msg) => serde_json::to_value(msg),
            StoreEntry::Marker(marker) => serde_json::to_value(marker),
        };
        let value = value.map_err(io::Error::other)?;
        serde_json::to_string(&value).map_err(io::Error::other)
    }

    /// Ensure the parent directory of the session file exists.
    async fn ensure_parent(&self, path: &std::path::Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }

    /// Write the metadata header line to a file, extracting title from first user message.
    async fn write_metadata_header(
        file: &mut File,
        id: &str,
        first_messages: &[Message],
    ) -> io::Result<()> {
        let metadata = SessionMetadata {
            metadata_type: "session".to_string(),
            session_id: id.to_string(),
            created_at: chrono::Utc::now(),
            title: extract_title(first_messages),
        };
        let json = serde_json::to_string(&metadata).map_err(io::Error::other)?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }
}

impl SessionStore for FsSessionStore {
    type Error = io::Error;

    async fn create(&self, id: &str, first_messages: &[Message]) -> Result<(), Self::Error> {
        let path = self.session_path(id);
        self.ensure_parent(&path).await?;

        // Write atomically: metadata + all messages in one go.
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await?;

        Self::write_metadata_header(&mut file, id, first_messages).await?;

        for msg in first_messages {
            let json = Self::serialize_entry(&StoreEntry::Message(msg.clone()))?;
            file.write_all(json.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<(SessionMetadata, Vec<StoreEntry>)>, Self::Error> {
        let path = self.session_path(id);

        if !path.exists() {
            return Ok(None);
        }

        let file = File::open(&path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // First line must be metadata
        let metadata_line = lines
            .next_line()
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty JSONL file"))?;

        let metadata: SessionMetadata = serde_json::from_str(&metadata_line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse metadata: {e}"),
            )
        })?;

        let mut entries = Vec::new();
        let mut line_num = 0usize;
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                line_num += 1;
                continue;
            }
            if let Some(entry) = Self::parse_entry(&line) {
                entries.push(entry);
            } else {
                log::warn!(
                    "Skipping corrupt/unrecognized line {} in session {}",
                    line_num + 2,
                    id
                );
            }
            line_num += 1;
        }

        if entries.is_empty() {
            return Ok(None);
        }

        Ok(Some((metadata, entries)))
    }

    async fn append(&self, id: &str, entries: &[StoreEntry]) -> Result<(), Self::Error> {
        let path = self.session_path(id);
        self.ensure_parent(&path).await?;

        // If file doesn't exist yet, create it with metadata header first
        if !path.exists() {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .await?;
            Self::write_metadata_header(&mut file, id, &[]).await?;
        }

        // Append entries
        let mut file = OpenOptions::new().append(true).open(&path).await?;
        for entry in entries {
            let json = Self::serialize_entry(entry)?;
            file.write_all(json.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn replace_entries(&self, id: &str, entries: &[StoreEntry]) -> Result<(), Self::Error> {
        let path = self.session_path(id);

        // Read existing metadata
        let metadata = if path.exists() {
            let content = fs::read_to_string(&path).await?;
            let first_line = content
                .lines()
                .next()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty JSONL file"))?;
            serde_json::from_str::<SessionMetadata>(first_line).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to parse metadata: {e}"),
                )
            })?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Session '{id}' not found"),
            ));
        };

        // Truncate and rewrite: metadata + new entries
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await?;

        let json = serde_json::to_string(&metadata).map_err(io::Error::other)?;
        file.write_all(json.as_bytes()).await?;
        file.write_all(b"\n").await?;

        for entry in entries {
            let json = Self::serialize_entry(entry)?;
            file.write_all(json.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn list(&self) -> Result<Vec<SessionInfo>, Self::Error> {
        let mut sessions = Vec::new();

        let mut read_dir = match fs::read_dir(&self.base_path).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(sessions);
            }
            Err(e) => return Err(e),
        };

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            // Read metadata (first line) and count non-empty subsequent lines
            let file = match File::open(&path).await {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut reader = BufReader::new(file);

            let mut metadata_line = String::new();
            if reader.read_line(&mut metadata_line).await? == 0 {
                continue; // empty file
            }
            if metadata_line.trim().is_empty() {
                continue;
            }

            let metadata: SessionMetadata = match serde_json::from_str(&metadata_line) {
                Ok(m) => m,
                Err(_) => continue, // skip corrupt files
            };

            let mut message_count = 0usize;
            let mut line_buf = String::new();
            while reader.read_line(&mut line_buf).await? != 0 {
                if !line_buf.trim().is_empty() {
                    message_count += 1;
                }
                line_buf.clear();
            }

            // Filter out sessions with zero entries
            if message_count == 0 {
                continue;
            }

            sessions.push(SessionInfo {
                id: metadata.session_id,
                message_count,
                last_active: metadata.created_at,
                title: metadata.title,
            });
        }

        // Sort newest first by created_at
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active));
        Ok(sessions)
    }

    async fn delete(&self, id: &str) -> Result<(), Self::Error> {
        let path = self.session_path(id);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }
}

/// Final safety net after repair_messages(). Walks the message list and strips any
/// `Assistant(ToolCall)` that is not immediately followed by a `User` containing
/// matching ToolResults for ALL its IDs. Logs a warn! for each violation found.
/// Does NOT inject synthetic results — only strips and warns.
pub(crate) fn validate_tool_call_adjacency(messages: Vec<Message>) -> Vec<Message> {
    use crate::types::{AssistantContent, ToolCallId, UserContent};
    use std::collections::HashSet;

    // We loop until no violations remain; stripping one pair may expose another.
    let mut messages = messages;
    loop {
        // Find the first Assistant message whose ToolCall IDs are not all
        // covered by the immediately following User message.
        let violation_ids: Option<HashSet<ToolCallId>> =
            messages.iter().enumerate().find_map(|(i, msg)| {
                let Message::Assistant { content, .. } = msg else {
                    return None;
                };
                let call_ids: HashSet<ToolCallId> = content
                    .iter()
                    .filter_map(|item| match item {
                        AssistantContent::ToolCall(tc) => Some(tc.id.clone()),
                        _ => None,
                    })
                    .collect();
                if call_ids.is_empty() {
                    return None;
                }
                let next_result_ids: HashSet<ToolCallId> = match messages.get(i + 1) {
                    Some(Message::User { content }) => content
                        .iter()
                        .filter_map(|item| match item {
                            UserContent::ToolResult(tr) => Some(tr.call.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => HashSet::new(),
                };
                if call_ids.is_subset(&next_result_ids) {
                    None
                } else {
                    Some(call_ids)
                }
            });

        let Some(bad_ids) = violation_ids else {
            // No violations — we are done.
            break;
        };

        // Log and strip the offending pair.
        for id in &bad_ids {
            log::warn!(
                "validate_tool_call_adjacency: stripped non-adjacent ToolCall/ToolResult pair id={}",
                id
            );
        }

        messages = messages
            .into_iter()
            .filter_map(|msg| match msg {
                Message::Assistant { id, content } => {
                    let items: Vec<crate::types::AssistantContent> = content
                        .into_iter()
                        .filter(|item| match item {
                            AssistantContent::ToolCall(tc) => !bad_ids.contains(&tc.id),
                            _ => true,
                        })
                        .collect();
                    if items.is_empty() {
                        None
                    } else {
                        Some(Message::Assistant { id, content: items })
                    }
                }
                Message::User { content } => {
                    let items: Vec<crate::types::UserContent> = content
                        .into_iter()
                        .filter(|item| match item {
                            UserContent::ToolResult(tr) => !bad_ids.contains(&tr.call),
                            _ => true,
                        })
                        .collect();
                    if items.is_empty() {
                        None
                    } else {
                        Some(Message::User { content: items })
                    }
                }
                system @ Message::System { .. } => Some(system),
            })
            .collect();
    }

    messages
}

/// Extracts the LLM context from a sequence of store entries.
///
/// If there are compaction markers, uses the **last** marker to determine context:
/// - Prepends the marker's summary as a system message (if non-empty)
/// - Includes all messages after the marker (for SlidingSummary, only post-compaction
///   new messages appear here; for SlidingWindow/TokenTruncate, kept messages may also
///   be re-appended after the marker)
///
/// If there are no markers, returns all messages.
pub fn extract_llm_context(entries: &[StoreEntry]) -> Vec<Message> {
    let last_marker_idx = entries
        .iter()
        .rposition(|e| matches!(e, StoreEntry::Marker(_)));

    let messages = match last_marker_idx {
        Some(idx) => {
            // The index came from `rposition` matching a Marker, so this arm
            // is always a Marker. Treating a non-Marker defensively yields an
            // empty context rather than panicking.
            let marker = match &entries[idx] {
                StoreEntry::Marker(m) => m,
                _ => {
                    log::warn!("extract_llm_context: expected Marker at index {idx}",);
                    return Vec::new();
                }
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
    };

    // Repair the conversation: remove empty messages, fix dangling tool call/result
    // pairs, merge consecutive same-role messages, and trim trailing orphan user messages.
    let (repaired, issues) = super::repair::repair_messages(messages);
    for issue in &issues {
        log::warn!("conversation repair: {}", issue);
    }
    // Final safety net: validate adjacency and strip any remaining violations.
    validate_tool_call_adjacency(repaired)
}
