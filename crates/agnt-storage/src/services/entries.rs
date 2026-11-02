//! The per-entry attribution: rig's `MessageKey` of each entry a turn persisted,
//! pointing at the turn that persisted it.

// region:    --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::storage::Storage;

// endregion: --- Modules

// region:    --- Types

/// rig's `MessageKey { made, id }`, as this database stores it.
///
/// The key is rig's identity for one persisted entry: the instant the entry was
/// made, and the id the store gave it. It is kept as explicit columns, so a rig
/// change to that identity becomes a migration and not a rewrite (D15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageKey {
    /// rig's `EntryRef.made`, in milliseconds since the Unix epoch.
    pub made: i64,
    /// rig's `MessageId`, the store's id for the entry.
    pub id: i64,
}

/// One entry's attribution: the key, and the turn that persisted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryTurn {
    /// The entry rig persisted.
    pub key: MessageKey,
    /// The turn the entry was persisted under.
    pub turn_seq: i64,
}

/// The entry attribution of one database.
pub struct Entries {
    storage: Arc<Storage>,
}

// endregion: --- Types

// region:    --- Constructors

impl Entries {
    /// The entry service over `storage`.
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { storage }
    }
}

// endregion: --- Constructors

// region:    --- Operations

impl Entries {
    /// Records `keys` as the entries `turn_seq` persisted.
    ///
    /// A key that already has a row takes the new attribution; a key whose turn
    /// is not in `turns` is refused by the foreign key, so an edge never points
    /// at a turn that was never recorded.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone, the turn edge does not hold, or
    /// SQLite refuses the write.
    pub async fn record(
        &self,
        session_id: String,
        turn_seq: i64,
        keys: Vec<MessageKey>,
    ) -> Result<()> {
        self.storage
            .call(Record {
                session_id,
                turn_seq,
                keys,
            })
            .await?
            .map_err(StorageError::from)
    }

    /// The attributions `session_id` holds.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone or SQLite refuses the read.
    pub async fn read(&self, session_id: &str) -> Result<Vec<EntryTurn>> {
        self.storage
            .call(Read(session_id.to_owned()))
            .await?
            .map_err(StorageError::from)
    }
}

// endregion: --- Operations

// region:    --- Support

/// Writes one turn's entry keys, replacing the row of a key already recorded.
struct Record {
    session_id: String,
    turn_seq: i64,
    keys: Vec<MessageKey>,
}

impl Work for Record {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        for key in &self.keys {
            sqlx::query(
                "INSERT INTO entry_turns (session_id, made, entry_id, turn_seq)\n                 VALUES (?1, ?2, ?3, ?4)\n                 ON CONFLICT(session_id, made, entry_id) DO UPDATE SET\n                     turn_seq = excluded.turn_seq",
            )
            .bind(&self.session_id)
            .bind(key.made)
            .bind(key.id)
            .bind(self.turn_seq)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}

/// Answers one session's entry attributions, in key order.
struct Read(String);

impl Work for Read {
    type Output = sqlx::Result<Vec<EntryTurn>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT made, entry_id, turn_seq FROM entry_turns\n             WHERE session_id = ?1 ORDER BY turn_seq, made, entry_id",
        )
        .bind(self.0)
        .fetch_all(&pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(made, id, turn_seq)| EntryTurn {
                key: MessageKey { made, id },
                turn_seq,
            })
            .collect())
    }
}

// endregion: --- Support
