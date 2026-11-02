//! The settled tool-call statuses: one row per call, with the status it settled
//! with and when.

// region:    --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::storage::Storage;

// endregion: --- Modules

// region:    --- Types

/// The status a call settled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// The call ran and produced a result.
    Success,
    /// The call ran and failed.
    Failed,
    /// The call never reached a result: the turn's cancel stopped it.
    Cancelled,
}

/// One settled call's facts, every one of them known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFact {
    /// The session the call belongs to.
    pub session_id: String,
    /// The call's id, as the turn knows it.
    pub call: String,
    /// The turn the call ran under.
    pub turn_seq: i64,
    /// The status it settled with.
    pub status: ToolCallStatus,
    /// When it settled, in milliseconds since the Unix epoch.
    pub settled_millis: i64,
}

/// The call statuses of one database.
pub struct ToolCalls {
    storage: Arc<Storage>,
}

// endregion: --- Types

// region:    --- Constructors

impl ToolCalls {
    /// The tool-call service over `storage`.
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

// endregion: --- Constructors

// region:    --- Operations

impl ToolCalls {
    /// Records `fact`, replacing the row the same call already holds.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone, the turn edge does not hold, or
    /// SQLite refuses the write.
    pub async fn record(&self, fact: CallFact) -> Result<()> {
        self.storage
            .call(Record(fact))
            .await?
            .map_err(StorageError::from)
    }

    /// The calls `session_id` settled.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone, SQLite refuses the read, or a
    /// stored status names none of the three outcomes.
    pub async fn read(&self, session_id: &str) -> Result<Vec<CallFact>> {
        self.storage.call(Read(session_id.to_owned())).await?
    }
}

// endregion: --- Operations

// region:    --- Support

/// One `tool_calls` row, as the read selects it.
type CallRow = (String, String, i64, String, i64);

impl ToolCallStatus {
    /// The value the `status` column's CHECK accepts.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// The status `stored` names; `None` when it names none of them.
    fn parse(stored: &str) -> Option<Self> {
        match stored {
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Writes one settled call, replacing the row of the same call.
struct Record(CallFact);

impl Work for Record {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let fact = self.0;
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO tool_calls (session_id, call, turn_seq, status, settled_millis)\n             VALUES (?1, ?2, ?3, ?4, ?5)\n             ON CONFLICT(session_id, call) DO UPDATE SET\n                 turn_seq = excluded.turn_seq,\n                 status = excluded.status,\n                 settled_millis = excluded.settled_millis",
        )
        .bind(&fact.session_id)
        .bind(&fact.call)
        .bind(fact.turn_seq)
        .bind(fact.status.as_str())
        .bind(fact.settled_millis)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
}

/// Answers one session's settled calls, in call order.
struct Read(String);

impl Work for Read {
    type Output = crate::error::Result<Vec<CallFact>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<CallRow> = sqlx::query_as::<_, CallRow>(
            "SELECT session_id, call, turn_seq, status, settled_millis\n             FROM tool_calls WHERE session_id = ?1 ORDER BY call",
        )
        .bind(self.0)
        .fetch_all(&pool)
        .await
        .map_err(StorageError::from)?;
        let mut facts = Vec::with_capacity(rows.len());
        for (session_id, call, turn_seq, status, settled_millis) in rows {
            let Some(status) = ToolCallStatus::parse(&status) else {
                return Err(StorageError::Custom(format!(
                    "the tool call '{call}' stores the unknown status '{status}'"
                )));
            };
            facts.push(CallFact {
                session_id,
                call,
                turn_seq,
                status,
                settled_millis,
            });
        }
        Ok(facts)
    }
}

// endregion: --- Support
