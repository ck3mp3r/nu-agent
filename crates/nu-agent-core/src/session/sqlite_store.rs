use super::store::{CompactionMarker, SessionStore, StoreEntry};
use super::{SessionInfo, SessionMetadata, extract_title};
use crate::types::Message;
use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

/// Sqlite-based implementation of SessionStore using sqlx (async-native).
///
/// Uses a single SQLite database file with the following schema:
/// - `sessions` table: id, created_at, title
/// - `entries` table: session_id, seq, entry_type, data (JSON blob)
#[derive(Debug, Clone)]
pub struct SqliteSessionStore {
    pool: SqlitePool,
}

impl SqliteSessionStore {
    /// Create a new SqliteSessionStore with the given database path.
    ///
    /// Creates parent directories if they don't exist.
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        // Create parent directory if it doesn't exist (SQLite can create
        // the file but not the directory)
        if let Some(parent) = std::path::Path::new(db_path).parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Build connection options with create_if_missing enabled.
        // For ":memory:" use the in-memory URI; for file paths use sqlite:// prefix.
        let conn_str = if db_path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite://{db_path}")
        };
        let opts = SqliteConnectOptions::from_str(&conn_str)?.create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect_with(opts)
            .await?;

        // Run versioned migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }

    /// Create a new SqliteSessionStore from an existing pool.
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SessionStore for SqliteSessionStore {
    type Error = sqlx::Error;

    async fn create(&self, id: &str, first_messages: &[Message]) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await?;

        // Extract title from first user message
        let title = extract_title(first_messages).unwrap_or_default();

        // Insert session metadata
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT OR IGNORE INTO sessions (id, created_at, title) VALUES (?, ?, ?)")
            .bind(id)
            .bind(&now)
            .bind(&title)
            .execute(&mut *tx)
            .await?;

        // Insert messages
        for (i, msg) in first_messages.iter().enumerate() {
            let data = serde_json::to_string(msg)
                .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize message: {e}")))?;
            sqlx::query(
                "INSERT OR REPLACE INTO entries (session_id, seq, entry_type, data) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(i as i64)
            .bind("message")
            .bind(&data)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    async fn load(
        &self,
        id: &str,
    ) -> Result<Option<(SessionMetadata, Vec<StoreEntry>)>, Self::Error> {
        // Check if session exists
        let session_row: Option<(String, String, String)> =
            sqlx::query_as("SELECT id, created_at, title FROM sessions WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        let (session_id, created_at_str, title) = match session_row {
            Some(row) => row,
            None => return Ok(None),
        };

        let created_at: DateTime<Utc> = created_at_str.parse().unwrap_or_else(|_| Utc::now());

        // Load entries
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT entry_type, data FROM entries WHERE session_id = ? ORDER BY seq",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut entries = Vec::new();
        for (entry_type, data) in &rows {
            match entry_type.as_str() {
                "message" => {
                    if let Ok(msg) = serde_json::from_str::<Message>(data) {
                        entries.push(StoreEntry::Message(msg));
                    }
                }
                "compaction_marker" => {
                    if let Ok(marker) = serde_json::from_str::<CompactionMarker>(data) {
                        entries.push(StoreEntry::Marker(marker));
                    }
                }
                _ => {
                    log::warn!("Unknown entry type '{}' in session {}", entry_type, id);
                }
            }
        }

        let metadata = SessionMetadata {
            metadata_type: "session".to_string(),
            session_id,
            created_at,
            title: if title.is_empty() { None } else { Some(title) },
        };

        Ok(Some((metadata, entries)))
    }

    async fn append(&self, id: &str, entries: &[StoreEntry]) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await?;

        // Ensure session exists
        let exists: bool =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE id = ?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?
                > 0;

        if !exists {
            let now = Utc::now().to_rfc3339();
            sqlx::query("INSERT OR IGNORE INTO sessions (id, created_at, title) VALUES (?, ?, ?)")
                .bind(id)
                .bind(&now)
                .bind("")
                .execute(&mut *tx)
                .await?;
        }

        // Get next sequence number
        let next_seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM entries WHERE session_id = ?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        for (i, entry) in entries.iter().enumerate() {
            let (entry_type, data) = match entry {
                StoreEntry::Message(msg) => {
                    let data = serde_json::to_string(msg).map_err(|e| {
                        sqlx::Error::Protocol(format!("Failed to serialize message: {e}"))
                    })?;
                    ("message", data)
                }
                StoreEntry::Marker(marker) => {
                    let data = serde_json::to_string(marker).map_err(|e| {
                        sqlx::Error::Protocol(format!("Failed to serialize marker: {e}"))
                    })?;
                    ("compaction_marker", data)
                }
            };

            sqlx::query(
                "INSERT OR REPLACE INTO entries (session_id, seq, entry_type, data) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(next_seq + i as i64)
            .bind(entry_type)
            .bind(&data)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    async fn replace_entries(&self, id: &str, entries: &[StoreEntry]) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await?;

        // Delete existing entries
        sqlx::query("DELETE FROM entries WHERE session_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        // Insert new entries
        for (i, entry) in entries.iter().enumerate() {
            let (entry_type, data) = match entry {
                StoreEntry::Message(msg) => {
                    let data = serde_json::to_string(msg).map_err(|e| {
                        sqlx::Error::Protocol(format!("Failed to serialize message: {e}"))
                    })?;
                    ("message", data)
                }
                StoreEntry::Marker(marker) => {
                    let data = serde_json::to_string(marker).map_err(|e| {
                        sqlx::Error::Protocol(format!("Failed to serialize marker: {e}"))
                    })?;
                    ("compaction_marker", data)
                }
            };

            sqlx::query(
                "INSERT INTO entries (session_id, seq, entry_type, data) VALUES (?, ?, ?, ?)",
            )
            .bind(id)
            .bind(i as i64)
            .bind(entry_type)
            .bind(&data)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    async fn list(&self) -> Result<Vec<SessionInfo>, Self::Error> {
        let rows: Vec<(String, String, i64, String)> = sqlx::query_as(
            "SELECT s.id, s.created_at, COUNT(e.seq) as message_count, s.title
             FROM sessions s
             LEFT JOIN entries e ON e.session_id = s.id
             GROUP BY s.id
             HAVING message_count > 0
             ORDER BY s.created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let sessions = rows
            .into_iter()
            .filter_map(|(id, created_at_str, message_count, title)| {
                let created_at: DateTime<Utc> = created_at_str.parse().ok()?;
                Some(SessionInfo {
                    id,
                    message_count: message_count as usize,
                    last_active: created_at,
                    title: if title.is_empty() { None } else { Some(title) },
                })
            })
            .collect();

        Ok(sessions)
    }

    async fn delete(&self, id: &str) -> Result<(), Self::Error> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM entries WHERE session_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await
    }
}

#[cfg(test)]
#[path = "sqlite_store_test.rs"]
mod sqlite_store_test;
