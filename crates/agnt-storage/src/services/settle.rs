//! The settle unit: one turn's facts, written as one work unit.
//!
//! A turn's settle is one logical write: its row, the keys of the entries it
//! persisted, and the statuses its calls settled with belong together. Written
//! through one unit, they land in one transaction, so a settle that fails on
//! any row leaves none of them behind and a reader never sees a turn without
//! the keys or the calls that belong to it.

// region:    --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::services::entries::MessageKey;
use crate::services::tool_calls::CallFact;
use crate::services::turns::TurnFacts;
use crate::storage::Storage;

// endregion: --- Modules

// region:    --- Types

/// One settled turn: its facts, the keys it persisted, and its calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleRecord {
    /// The turn's own facts.
    pub turn: TurnFacts,
    /// rig's key for each entry the turn persisted.
    pub keys: Vec<MessageKey>,
    /// The status each of the turn's calls settled with.
    pub calls: Vec<CallFact>,
}

/// The settle unit of one database.
pub struct Settle {
    storage: Arc<Storage>,
}

// endregion: --- Types

// region:    --- Constructors

impl Settle {
    /// The settle service over `storage`.
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

// endregion: --- Constructors

// region:    --- Operations

impl Settle {
    /// Records `record` whole: one transaction, or nothing.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone, a row names a turn that is not
    /// recorded, or SQLite refuses any row of the unit.
    pub async fn record(&self, record: SettleRecord) -> Result<()> {
        self.storage
            .call(Record(record))
            .await?
            .map_err(StorageError::from)
    }
}

// endregion:    --- Operations

// region:    --- Support

/// Writes one settle record in one transaction.
struct Record(SettleRecord);

impl Work for Record {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let record = self.0;
        let turn = &record.turn;
        let mut tx = pool.begin().await?;
        sqlx::query(
            "INSERT INTO turns (session_id, turn_seq, submitted_millis, settled_millis, model, agent)\n             VALUES (?1, ?2, ?3, ?4, ?5, ?6)\n             ON CONFLICT(session_id, turn_seq) DO UPDATE SET\n                 submitted_millis = excluded.submitted_millis,\n                 settled_millis = excluded.settled_millis,\n                 model = excluded.model,\n                 agent = excluded.agent",
        )
        .bind(&turn.session_id)
        .bind(turn.turn_seq)
        .bind(turn.submitted_millis)
        .bind(turn.settled_millis)
        .bind(&turn.model)
        .bind(&turn.agent)
        .execute(&mut *tx)
        .await?;
        for key in &record.keys {
            sqlx::query(
                "INSERT INTO entry_turns (session_id, made, entry_id, turn_seq)\n                 VALUES (?1, ?2, ?3, ?4)\n                 ON CONFLICT(session_id, made, entry_id) DO UPDATE SET\n                     turn_seq = excluded.turn_seq",
            )
            .bind(&turn.session_id)
            .bind(key.made)
            .bind(key.id)
            .bind(turn.turn_seq)
            .execute(&mut *tx)
            .await?;
        }
        for call in &record.calls {
            sqlx::query(
                "INSERT INTO tool_calls (session_id, call, turn_seq, status, settled_millis)\n                 VALUES (?1, ?2, ?3, ?4, ?5)\n                 ON CONFLICT(session_id, call) DO UPDATE SET\n                     turn_seq = excluded.turn_seq,\n                     status = excluded.status,\n                     settled_millis = excluded.settled_millis",
            )
            .bind(&call.session_id)
            .bind(&call.call)
            .bind(call.turn_seq)
            .bind(call.status.as_str())
            .bind(call.settled_millis)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}

// endregion: --- Support
