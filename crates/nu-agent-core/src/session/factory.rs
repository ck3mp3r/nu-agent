use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::store::{SessionStore, StoreEntry};
use super::{FsSessionStore, SessionInfo, SessionMetadata};
use crate::session::sqlite_store::SqliteSessionStore;
use crate::types::Message;

/// The type of session store backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StoreType {
    /// SQLite-backed session store.
    #[default]
    #[serde(rename = "sqlite")]
    Sqlite,
    /// JSONL file-backed session store.
    #[serde(rename = "jsonl")]
    Jsonl,
}

impl FromStr for StoreType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sqlite" => Ok(StoreType::Sqlite),
            "jsonl" => Ok(StoreType::Jsonl),
            other => Err(format!(
                "Unknown store type: '{other}'. Expected 'sqlite' or 'jsonl'."
            )),
        }
    }
}

/// Errors that can occur when using a session store.
#[derive(Debug)]
pub enum StoreError {
    /// An I/O error occurred.
    Io(std::io::Error),
    /// A SQLite error occurred.
    Sqlite(sqlx::Error),
    /// A JSON serialization/deserialization error occurred.
    Json(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "I/O error: {e}"),
            StoreError::Sqlite(e) => write!(f, "SQLite error: {e}"),
            StoreError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io(e) => Some(e),
            StoreError::Sqlite(e) => Some(e),
            StoreError::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

/// A unified session store that dispatches to either a JSONL or SQLite backend.
#[derive(Debug, Clone)]
pub enum SessionStoreImpl {
    /// JSONL file-backed session store.
    Fs(FsSessionStore),
    /// SQLite-backed session store.
    Sqlite(SqliteSessionStore),
}

impl SessionStoreImpl {
    /// Returns the `StoreType` of this session store.
    pub fn store_type(&self) -> StoreType {
        match self {
            SessionStoreImpl::Fs(_) => StoreType::Jsonl,
            SessionStoreImpl::Sqlite(_) => StoreType::Sqlite,
        }
    }
}

impl SessionStore for SessionStoreImpl {
    type Error = StoreError;

    fn create(
        &self,
        id: &str,
        first_messages: &[Message],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let id = id.to_string();
        let messages = first_messages.to_vec();
        async move {
            match self {
                SessionStoreImpl::Fs(s) => s.create(&id, &messages).await.map_err(StoreError::from),
                SessionStoreImpl::Sqlite(s) => {
                    s.create(&id, &messages).await.map_err(StoreError::from)
                }
            }
        }
    }

    fn load(
        &self,
        id: &str,
    ) -> impl std::future::Future<
        Output = Result<Option<(SessionMetadata, Vec<StoreEntry>)>, Self::Error>,
    > + Send {
        let id = id.to_string();
        async move {
            match self {
                SessionStoreImpl::Fs(s) => s.load(&id).await.map_err(StoreError::from),
                SessionStoreImpl::Sqlite(s) => s.load(&id).await.map_err(StoreError::from),
            }
        }
    }

    fn append(
        &self,
        id: &str,
        entries: &[StoreEntry],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let id = id.to_string();
        let entries = entries.to_vec();
        async move {
            match self {
                SessionStoreImpl::Fs(s) => s.append(&id, &entries).await.map_err(StoreError::from),
                SessionStoreImpl::Sqlite(s) => {
                    s.append(&id, &entries).await.map_err(StoreError::from)
                }
            }
        }
    }

    fn replace_entries(
        &self,
        id: &str,
        entries: &[StoreEntry],
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let id = id.to_string();
        let entries = entries.to_vec();
        async move {
            match self {
                SessionStoreImpl::Fs(s) => s
                    .replace_entries(&id, &entries)
                    .await
                    .map_err(StoreError::from),
                SessionStoreImpl::Sqlite(s) => s
                    .replace_entries(&id, &entries)
                    .await
                    .map_err(StoreError::from),
            }
        }
    }

    async fn list(&self) -> Result<Vec<SessionInfo>, Self::Error> {
        match self {
            SessionStoreImpl::Fs(s) => s.list().await.map_err(StoreError::from),
            SessionStoreImpl::Sqlite(s) => s.list().await.map_err(StoreError::from),
        }
    }

    fn delete(
        &self,
        id: &str,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let id = id.to_string();
        async move {
            match self {
                SessionStoreImpl::Fs(s) => s.delete(&id).await.map_err(StoreError::from),
                SessionStoreImpl::Sqlite(s) => s.delete(&id).await.map_err(StoreError::from),
            }
        }
    }
}

/// Create a session store of the given type.
///
/// For `Jsonl`, uses `xdg::cache_dir().join("nu-agent").join("sessions")` as the base path.
/// For `Sqlite`, uses `xdg::cache_dir().join("nu-agent").join("sessions.db")` as the database path.
pub async fn create_store(store_type: StoreType) -> Result<SessionStoreImpl, StoreError> {
    match store_type {
        StoreType::Jsonl => {
            let path = crate::utils::xdg::cache_dir()
                .map_err(|e| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Failed to resolve XDG cache directory: {e}"),
                    ))
                })?
                .join("nu-agent")
                .join("sessions");
            Ok(SessionStoreImpl::Fs(FsSessionStore::new(path)))
        }
        StoreType::Sqlite => {
            let path = crate::utils::xdg::cache_dir()
                .map_err(|e| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Failed to resolve XDG cache directory: {e}"),
                    ))
                })?
                .join("nu-agent")
                .join("sessions.db");
            let path_str = path
                .to_str()
                .ok_or_else(|| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Non-UTF-8 path for SQLite database",
                    ))
                })?
                .to_string();
            let store = SqliteSessionStore::new(&path_str)
                .await
                .map_err(StoreError::from)?;
            Ok(SessionStoreImpl::Sqlite(store))
        }
    }
}
