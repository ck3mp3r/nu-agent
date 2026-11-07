//! The per-entry attribution: rig's `MessageKey` of each entry a turn persisted,
//! pointing at the turn that persisted it. The transcript rows the store keeps
//! are the message rows of a session, so they land through the same service.

// region:    --- Modules

use std::sync::Arc;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::storage::Storage;
use crate::transcript::{TranscriptRecord, TranscriptRow, TranscriptStamp};

use super::tool_calls::ToolCallStatus;

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

    /// Records `rows` as their sessions' transcript, one row per position.
    ///
    /// A row that already occupies a position of a session is replaced, so a
    /// fold that re-reads a session lands each row once. Each row's identity is
    /// derived from its own content, so the table never holds an id nobody
    /// derived.
    ///
    /// The live fold's write-side wiring is not here yet: the producer that
    /// folds the session stream calls this, and until it does the table stays as
    /// empty as a session that has not folded.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone or SQLite refuses the write.
    pub async fn record_transcript(&self, rows: Vec<TranscriptRecord>) -> Result<()> {
        self.storage
            .call(RecordTranscript(rows))
            .await?
            .map_err(StorageError::from)
    }
}

// endregion: --- Operations

// region:    --- Support

/// Writes one set of transcript rows in one transaction.
struct RecordTranscript(Vec<TranscriptRecord>);

impl Work for RecordTranscript {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let mut tx = pool.begin().await?;
        for record in &self.0 {
            let id = record.id();
            let payload = Payload::of(record.row());
            sqlx::query(
                "INSERT INTO transcript (id, session_id, row_seq, kind, content, tool_name, tool_arguments, tool_result, tool_success, tool_status, stamp_millis, stamp_model, stamp_agent)\n                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)\n                 ON CONFLICT(session_id, row_seq) DO UPDATE SET\n                     id = excluded.id,\n                     kind = excluded.kind,\n                     content = excluded.content,\n                     tool_name = excluded.tool_name,\n                     tool_arguments = excluded.tool_arguments,\n                     tool_result = excluded.tool_result,\n                     tool_success = excluded.tool_success,\n                     tool_status = excluded.tool_status,\n                     stamp_millis = excluded.stamp_millis,\n                     stamp_model = excluded.stamp_model,\n                     stamp_agent = excluded.stamp_agent",
            )
            .bind(id.as_str())
            .bind(record.session())
            .bind(record.row_seq())
            .bind(kind_of(record.row()))
            .bind(&payload.content)
            .bind(&payload.tool_name)
            .bind(&payload.tool_arguments)
            .bind(&payload.tool_result)
            .bind(payload.tool_success)
            .bind(payload.tool_status.map(ToolCallStatus::as_str))
            .bind(payload.stamp_millis)
            .bind(&payload.stamp_model)
            .bind(&payload.stamp_agent)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
}

/// Answers one session's transcript rows, in fold order.
///
/// The store's one transcript read: the re-hydration path submits this unit and
/// no other fold stands between the table and the caller.
pub(crate) struct ReadTranscript {
    pub(crate) session_id: String,
}

/// One `transcript` row, as the read selects it.
type TranscriptRowTuple = (
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

impl Work for ReadTranscript {
    type Output = crate::error::Result<Vec<TranscriptRecord>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<TranscriptRowTuple> = sqlx::query_as(
            "SELECT id, session_id, row_seq, kind, content, tool_name, tool_arguments,\n                    tool_result, tool_success, tool_status, stamp_millis, stamp_model,\n                    stamp_agent\n             FROM transcript WHERE session_id = ?1 ORDER BY row_seq",
        )
        .bind(self.session_id)
        .fetch_all(&pool)
        .await
        .map_err(StorageError::from)?;
        let mut records = Vec::with_capacity(rows.len());
        for (
            id,
            session_id,
            row_seq,
            kind,
            content,
            tool_name,
            tool_arguments,
            tool_result,
            tool_success,
            tool_status,
            stamp_millis,
            stamp_model,
            stamp_agent,
        ) in rows
        {
            let row = Columns {
                kind,
                content,
                tool_name,
                tool_arguments,
                tool_result,
                tool_success,
                tool_status,
                stamp_millis,
                stamp_model,
                stamp_agent,
            }
            .row()?;
            let record = TranscriptRecord::new(session_id, row_seq, row);
            let derived = record.id();
            if derived.as_str() != id {
                return Err(StorageError::Custom(format!(
                    "the transcript row at position {row_seq} stores the id '{id}', which is not the id its content derives: '{}'",
                    derived.as_str()
                )));
            }
            records.push(record);
        }
        Ok(records)
    }
}

/// The `kind` column of `row`.
fn kind_of(row: &TranscriptRow) -> &'static str {
    match row {
        TranscriptRow::User { .. } => "user",
        TranscriptRow::Assistant { .. } => "assistant",
        TranscriptRow::System { .. } => "system",
        TranscriptRow::Tool { .. } => "tool",
        TranscriptRow::Compaction { .. } => "compaction",
    }
}

/// The payload columns of one transcript row.
///
/// Absent parts are `None`, never an invented value: a row kind that carries no
/// tool leaves every tool column NULL.
#[derive(Default)]
struct Payload {
    content: String,
    tool_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<bool>,
    tool_status: Option<ToolCallStatus>,
    stamp_millis: Option<i64>,
    stamp_model: Option<String>,
    stamp_agent: Option<String>,
}

impl Payload {
    /// `row` as the columns the `transcript` table stores.
    fn of(row: &TranscriptRow) -> Self {
        match row {
            TranscriptRow::User { content, stamp } => Self {
                content: content.clone(),
                stamp_millis: stamp.as_ref().map(|stamp| stamp.millis),
                stamp_model: stamp.as_ref().and_then(|stamp| stamp.model.clone()),
                stamp_agent: stamp.as_ref().and_then(|stamp| stamp.agent.clone()),
                ..Self::default()
            },
            TranscriptRow::Assistant { content } => Self {
                content: content.clone(),
                ..Self::default()
            },
            TranscriptRow::System { content } => Self {
                content: content.clone(),
                ..Self::default()
            },
            TranscriptRow::Compaction { content } => Self {
                content: content.clone(),
                ..Self::default()
            },
            TranscriptRow::Tool {
                name,
                arguments,
                result,
                success,
                status,
            } => Self {
                tool_name: Some(name.clone()),
                tool_arguments: arguments.clone(),
                tool_result: result.clone(),
                tool_success: *success,
                tool_status: *status,
                ..Self::default()
            },
        }
    }
}

/// One `transcript` row, as the read selects it.
///
/// A status the table does not accept, an unknown `kind`, or a stamp identity
/// with no instant is a row this store cannot have written, so it is refused
/// instead of being handed out as a shape the write could not produce.
struct Columns {
    kind: String,
    content: String,
    tool_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<i64>,
    tool_status: Option<String>,
    stamp_millis: Option<i64>,
    stamp_model: Option<String>,
    stamp_agent: Option<String>,
}

impl Columns {
    /// The row these columns describe.
    fn row(self) -> crate::error::Result<TranscriptRow> {
        let stamp = stamp_of(self.stamp_millis, self.stamp_model, self.stamp_agent)?;
        let row = match self.kind.as_str() {
            "user" => TranscriptRow::User {
                content: self.content,
                stamp,
            },
            "assistant" => TranscriptRow::Assistant {
                content: self.content,
            },
            "system" => TranscriptRow::System {
                content: self.content,
            },
            "compaction" => TranscriptRow::Compaction {
                content: self.content,
            },
            "tool" => TranscriptRow::Tool {
                name: self.tool_name.unwrap_or_default(),
                arguments: self.tool_arguments,
                result: self.tool_result,
                success: self.tool_success.map(|success| success != 0),
                status: match self.tool_status {
                    Some(stored) => Some(parse_status(&stored)?),
                    None => None,
                },
            },
            unknown => {
                return Err(StorageError::Custom(format!(
                    "the transcript stores the unknown kind '{unknown}'"
                )));
            }
        };
        Ok(row)
    }
}

/// The status `stored` names, refused when it names none of them.
fn parse_status(stored: &str) -> crate::error::Result<ToolCallStatus> {
    ToolCallStatus::parse(stored).ok_or_else(|| {
        StorageError::Custom(format!(
            "the transcript stores the unknown tool status '{stored}'"
        ))
    })
}

/// The stamp the optional stamp columns carry.
///
/// A stamp exists only for a turn that recorded an instant, so identities with
/// no instant are refused rather than folded into a stamp the write never
/// produces.
fn stamp_of(
    millis: Option<i64>,
    model: Option<String>,
    agent: Option<String>,
) -> crate::error::Result<Option<TranscriptStamp>> {
    match millis {
        Some(millis) => Ok(Some(TranscriptStamp {
            millis,
            model,
            agent,
        })),
        None if model.is_none() && agent.is_none() => Ok(None),
        None => Err(StorageError::Custom(
            "the transcript stores a stamp identity with no instant".to_owned(),
        )),
    }
}

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
