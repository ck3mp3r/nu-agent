//! The turn facts: one row per nu-agent turn, written when it settles.

// region:    --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::storage::Storage;

// endregion: --- Modules

// region:    --- Types

/// One settled turn's facts, every one of them known.
///
/// The caller supplies all six: a turn that reached its settle point has them,
/// and a value this type cannot hold is a caller bug the table's `NOT NULL`
/// columns reject loudly rather than a row that reads back half-known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFacts {
    /// The session the turn belongs to.
    pub session_id: String,
    /// The turn's ordinal within the session; the caller assigns it.
    pub turn_seq: i64,
    /// When the turn was submitted, in milliseconds since the Unix epoch.
    pub submitted_millis: i64,
    /// When the turn settled, in milliseconds since the Unix epoch.
    pub settled_millis: i64,
    /// The model identity the turn ran under, `vendor/model`.
    pub model: String,
    /// The persona that answered the turn.
    pub agent: String,
}

/// The turn facts of one database.
pub struct Turns {
    storage: Arc<Storage>,
}

// endregion: --- Types

// region:    --- Constructors

impl Turns {
    /// The turns service over `storage`.
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

// endregion: --- Constructors

// region:    --- Operations

impl Turns {
    /// Records `facts`, replacing the row the same turn already holds.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone or SQLite refuses the write.
    pub async fn record(&self, facts: TurnFacts) -> Result<()> {
        self.storage
            .call(Record(facts))
            .await?
            .map_err(StorageError::from)
    }

    /// The turns of `session_id`, in turn order.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone or SQLite refuses the read.
    pub async fn read(&self, session_id: &str) -> Result<Vec<TurnFacts>> {
        self.storage
            .call(Read(session_id.to_owned()))
            .await?
            .map_err(StorageError::from)
    }
}

// endregion: --- Operations

// region:    --- Support

/// Writes one turn's facts, replacing the row of the same turn.
struct Record(TurnFacts);

impl Work for Record {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let facts = self.0;
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO turns (session_id, turn_seq, submitted_millis, settled_millis, model, agent)\n             VALUES (?1, ?2, ?3, ?4, ?5, ?6)\n             ON CONFLICT(session_id, turn_seq) DO UPDATE SET\n                 submitted_millis = excluded.submitted_millis,\n                 settled_millis = excluded.settled_millis,\n                 model = excluded.model,\n                 agent = excluded.agent",
        )
        .bind(&facts.session_id)
        .bind(facts.turn_seq)
        .bind(facts.submitted_millis)
        .bind(facts.settled_millis)
        .bind(&facts.model)
        .bind(&facts.agent)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
}

/// Answers one session's turns, in turn order.
struct Read(String);

impl Work for Read {
    type Output = sqlx::Result<Vec<TurnFacts>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(String, i64, i64, i64, String, String)> = sqlx::query_as(
            "SELECT session_id, turn_seq, submitted_millis, settled_millis, model, agent\n             FROM turns WHERE session_id = ?1 ORDER BY turn_seq",
        )
        .bind(self.0)
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(session_id, turn_seq, submitted_millis, settled_millis, model, agent)| {
                    TurnFacts {
                        session_id,
                        turn_seq,
                        submitted_millis,
                        settled_millis,
                        model,
                        agent,
                    }
                },
            )
            .collect())
    }
}

// endregion: --- Support
