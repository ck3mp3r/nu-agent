//! The transcript service: the rows a session folded, written once per position
//! and read back in fold order, each keyed by the identity its content derives.

// region:    --- Modules

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::engine::Work;
use crate::error::{Result, StorageError};
use crate::transcript::{
    MediaKind, MediaOwner, MediaSource, TranscriptRecord, TranscriptRow, TranscriptStamp,
};

use super::entries::Entries;
use super::tool_calls::ToolCallStatus;

// endregion: --- Modules

// region:    --- Operations

impl Entries {
    /// Records `rows` as their sessions' transcript, one row per position.
    ///
    /// A row that already occupies a position of a session is replaced, so a
    /// fold that re-reads a session lands each row once. A fold that keeps fewer
    /// blocks than the transcript holds removes the positions it no longer
    /// reaches: a folded log renumbers its rows from the first entry it keeps,
    /// and no stale row survives behind the new end. Each row's identity is
    /// derived from its own content, so the table never holds an id nobody
    /// derived.
    ///
    /// # Errors
    ///
    /// [`StorageError`] when the engine is gone or SQLite refuses the write.
    pub async fn record_transcript(&self, rows: Vec<TranscriptRecord>) -> Result<()> {
        self.storage()
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
        if self.0.is_empty() {
            return Ok(());
        }
        let mut tx = pool.begin().await?;
        let mut ends: HashMap<String, i64> = HashMap::new();
        for record in &self.0 {
            let id = record.id();
            let payload = Payload::of(record.row());
            let (source_kind, source, raw) = payload.media_source_columns();
            sqlx::query(
                "INSERT INTO transcript (id, session_id, row_seq, turn_seq, kind, content, tool_call_id, tool_name, tool_executed_name, tool_arguments, tool_result, tool_success, tool_status, stamp_millis, stamp_model, stamp_agent, media_owner, media_kind, media_source_kind, media_source, media_raw, media_text, media_type, media_detail, reasoning_redacted, opaque_item, opaque_replay)\n                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)\n                 ON CONFLICT(session_id, row_seq) DO UPDATE SET\n                     id = excluded.id,\n                     turn_seq = excluded.turn_seq,\n                     kind = excluded.kind,\n                     content = excluded.content,\n                     tool_call_id = excluded.tool_call_id,\n                     tool_name = excluded.tool_name,\n                     tool_executed_name = excluded.tool_executed_name,\n                     tool_arguments = excluded.tool_arguments,\n                     tool_result = excluded.tool_result,\n                     tool_success = excluded.tool_success,\n                     tool_status = excluded.tool_status,\n                     stamp_millis = excluded.stamp_millis,\n                     stamp_model = excluded.stamp_model,\n                     stamp_agent = excluded.stamp_agent,\n                     media_owner = excluded.media_owner,\n                     media_kind = excluded.media_kind,\n                     media_source_kind = excluded.media_source_kind,\n                     media_source = excluded.media_source,\n                     media_raw = excluded.media_raw,\n                     media_text = excluded.media_text,\n                     media_type = excluded.media_type,\n                     media_detail = excluded.media_detail,\n                     reasoning_redacted = excluded.reasoning_redacted,\n                     opaque_item = excluded.opaque_item,\n                     opaque_replay = excluded.opaque_replay",
            )
            .bind(id.as_str())
            .bind(record.session())
            .bind(record.row_seq())
            .bind(record.turn_seq())
            .bind(kind_of(record.row()))
            .bind(&payload.content)
            .bind(&payload.tool_call_id)
            .bind(&payload.tool_name)
            .bind(&payload.tool_executed_name)
            .bind(&payload.tool_arguments)
            .bind(&payload.tool_result)
            .bind(payload.tool_success.map(i64::from))
            .bind(payload.tool_status.map(ToolCallStatus::as_str))
            .bind(payload.stamp_millis)
            .bind(&payload.stamp_model)
            .bind(&payload.stamp_agent)
            .bind(payload.media_owner.map(MediaOwner::as_str))
            .bind(payload.media_kind.map(MediaKind::as_str))
            .bind(source_kind)
            .bind(source)
            .bind(raw)
            .bind(&payload.media_text)
            .bind(&payload.media_type)
            .bind(&payload.media_detail)
            .bind(payload.reasoning_redacted.map(i64::from))
            .bind(&payload.opaque_item)
            .bind(payload.opaque_replay.map(i64::from))
            .execute(&mut *tx)
            .await?;
            let end = ends.entry(record.session().to_owned()).or_insert(0);
            *end = (*end).max(record.row_seq() + 1);
        }
        // A fold renumbers its rows from the first block it keeps, so every
        // position past the new end belongs to a transcript that no longer is.
        for (session, end) in ends {
            sqlx::query("DELETE FROM transcript WHERE session_id = ?1 AND row_seq >= ?2")
                .bind(&session)
                .bind(end)
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

impl Work for ReadTranscript {
    type Output = crate::error::Result<Vec<TranscriptRecord>>;

    async fn run(self, pool: SqlitePool) -> Self::Output {
        let rows: Vec<StoredRow> = sqlx::query_as(
            "SELECT id, session_id, row_seq, turn_seq, kind, content, tool_call_id,\n                    tool_name, tool_executed_name, tool_arguments, tool_result,\n                    tool_success, tool_status, stamp_millis, stamp_model, stamp_agent,\n                    media_owner, media_kind, media_source_kind, media_source, media_raw,\n                    media_text, media_type, media_detail, reasoning_redacted,\n                    opaque_item, opaque_replay\n             FROM transcript WHERE session_id = ?1 ORDER BY row_seq",
        )
        .bind(self.session_id)
        .fetch_all(&pool)
        .await
        .map_err(StorageError::from)?;
        let mut records = Vec::with_capacity(rows.len());
        for stored in rows {
            records.push(stored.record()?);
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
        TranscriptRow::Media { .. } => "media",
        TranscriptRow::Reasoning { .. } => "reasoning",
        TranscriptRow::Opaque { .. } => "opaque",
    }
}

/// The payload columns of one transcript row.
///
/// Absent parts are `None`, never an invented value: a row kind that carries no
/// tool leaves every tool column NULL.
#[derive(Default)]
struct Payload {
    content: String,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_executed_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<bool>,
    tool_status: Option<ToolCallStatus>,
    stamp_millis: Option<i64>,
    stamp_model: Option<String>,
    stamp_agent: Option<String>,
    media_owner: Option<MediaOwner>,
    media_kind: Option<MediaKind>,
    media_source: Option<MediaSource>,
    media_text: Option<String>,
    media_type: Option<String>,
    media_detail: Option<String>,
    reasoning_redacted: Option<bool>,
    opaque_item: Option<String>,
    opaque_replay: Option<bool>,
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
                call_id,
                name,
                executed_name,
                arguments,
                result,
                success,
                status,
            } => Self {
                tool_call_id: Some(call_id.clone()),
                tool_name: Some(name.clone()),
                tool_executed_name: executed_name.clone(),
                tool_arguments: arguments.clone(),
                tool_result: result.clone(),
                tool_success: *success,
                tool_status: *status,
                ..Self::default()
            },
            TranscriptRow::Media {
                owner,
                kind,
                source,
                text,
                media_type,
                detail,
            } => Self {
                media_owner: Some(*owner),
                media_kind: Some(*kind),
                media_source: Some(source.clone()),
                media_text: text.clone(),
                media_type: media_type.clone(),
                media_detail: detail.clone(),
                ..Self::default()
            },
            TranscriptRow::Reasoning { text, redacted } => Self {
                content: text.clone(),
                reasoning_redacted: Some(*redacted),
                ..Self::default()
            },
            TranscriptRow::Opaque { item, replay } => Self {
                opaque_item: Some(item.clone()),
                opaque_replay: Some(*replay),
                ..Self::default()
            },
        }
    }

    /// The three media columns a source occupies: the token that names it, the
    /// text it carries, and the bytes when the source is the bytes themselves.
    fn media_source_columns(&self) -> (Option<&'static str>, Option<String>, Option<Vec<u8>>) {
        let Some(source) = &self.media_source else {
            return (None, None, None);
        };
        let value = match source {
            MediaSource::Url(value) | MediaSource::Base64(value) | MediaSource::FileId(value) => {
                Some(value.clone())
            }
            MediaSource::Raw(_) | MediaSource::Unknown => None,
        };
        let raw = match source {
            MediaSource::Raw(bytes) => Some(bytes.clone()),
            _ => None,
        };
        (Some(source.as_str()), value, raw)
    }
}

/// One `transcript` row, as the read selects it.
///
/// A status the table does not accept, an unknown `kind`, a media block that
/// names no source, or a stamp identity with no instant is a row this store
/// cannot have written, so it is refused instead of being handed out as a shape
/// the write could not produce.
struct StoredRow {
    id: String,
    session_id: String,
    row_seq: i64,
    turn_seq: Option<i64>,
    kind: String,
    content: String,
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tool_executed_name: Option<String>,
    tool_arguments: Option<String>,
    tool_result: Option<String>,
    tool_success: Option<i64>,
    tool_status: Option<String>,
    stamp_millis: Option<i64>,
    stamp_model: Option<String>,
    stamp_agent: Option<String>,
    media_owner: Option<String>,
    media_kind: Option<String>,
    media_source_kind: Option<String>,
    media_source: Option<String>,
    media_raw: Option<Vec<u8>>,
    media_text: Option<String>,
    media_type: Option<String>,
    media_detail: Option<String>,
    reasoning_redacted: Option<i64>,
    opaque_item: Option<String>,
    opaque_replay: Option<i64>,
}

impl<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow> for StoredRow {
    fn from_row(row: &'r sqlx::sqlite::SqliteRow) -> sqlx::Result<Self> {
        use sqlx::Row;
        Ok(Self {
            id: row.try_get("id")?,
            session_id: row.try_get("session_id")?,
            row_seq: row.try_get("row_seq")?,
            turn_seq: row.try_get("turn_seq")?,
            kind: row.try_get("kind")?,
            content: row.try_get("content")?,
            tool_call_id: row.try_get("tool_call_id")?,
            tool_name: row.try_get("tool_name")?,
            tool_executed_name: row.try_get("tool_executed_name")?,
            tool_arguments: row.try_get("tool_arguments")?,
            tool_result: row.try_get("tool_result")?,
            tool_success: row.try_get("tool_success")?,
            tool_status: row.try_get("tool_status")?,
            stamp_millis: row.try_get("stamp_millis")?,
            stamp_model: row.try_get("stamp_model")?,
            stamp_agent: row.try_get("stamp_agent")?,
            media_owner: row.try_get("media_owner")?,
            media_kind: row.try_get("media_kind")?,
            media_source_kind: row.try_get("media_source_kind")?,
            media_source: row.try_get("media_source")?,
            media_raw: row.try_get("media_raw")?,
            media_text: row.try_get("media_text")?,
            media_type: row.try_get("media_type")?,
            media_detail: row.try_get("media_detail")?,
            reasoning_redacted: row.try_get("reasoning_redacted")?,
            opaque_item: row.try_get("opaque_item")?,
            opaque_replay: row.try_get("opaque_replay")?,
        })
    }
}

impl StoredRow {
    /// The record these columns describe.
    ///
    /// The id the row came with is checked against the id its content derives:
    /// an id that disagrees with its content is the defect.
    fn record(self) -> crate::error::Result<TranscriptRecord> {
        let row = self.row()?;
        let record =
            TranscriptRecord::new(self.session_id, self.row_seq, row).with_turn_seq(self.turn_seq);
        let derived = record.id();
        if derived.as_str() != self.id {
            return Err(StorageError::Custom(format!(
                "the transcript row at position {} stores the id '{}', which is not the id its content derives: '{}'",
                record.row_seq(),
                self.id,
                derived.as_str()
            )));
        }
        Ok(record)
    }

    /// The row these columns describe.
    fn row(&self) -> crate::error::Result<TranscriptRow> {
        let stamp = stamp_of(
            self.stamp_millis,
            self.stamp_model.clone(),
            self.stamp_agent.clone(),
        )?;
        let row = match self.kind.as_str() {
            "user" => TranscriptRow::User {
                content: self.content.clone(),
                stamp,
            },
            "assistant" => TranscriptRow::Assistant {
                content: self.content.clone(),
            },
            "system" => TranscriptRow::System {
                content: self.content.clone(),
            },
            "compaction" => TranscriptRow::Compaction {
                content: self.content.clone(),
            },
            "tool" => TranscriptRow::Tool {
                call_id: self.tool_call_id.clone().ok_or_else(|| {
                    StorageError::Custom(
                        "the transcript stores a tool row with no call id".to_owned(),
                    )
                })?,
                name: self.tool_name.clone().unwrap_or_default(),
                executed_name: self.tool_executed_name.clone(),
                arguments: self.tool_arguments.clone(),
                result: self.tool_result.clone(),
                success: self.tool_success.map(|success| success != 0),
                status: match &self.tool_status {
                    Some(stored) => Some(parse_status(stored)?),
                    None => None,
                },
            },
            "media" => TranscriptRow::Media {
                owner: self
                    .media_owner
                    .as_deref()
                    .and_then(MediaOwner::parse)
                    .ok_or_else(|| {
                        StorageError::Custom(
                            "the transcript stores a media row with no owner".to_owned(),
                        )
                    })?,
                kind: self
                    .media_kind
                    .as_deref()
                    .and_then(MediaKind::parse)
                    .ok_or_else(|| {
                        StorageError::Custom(
                            "the transcript stores a media row with no kind".to_owned(),
                        )
                    })?,
                source: self
                    .media_source_kind
                    .as_deref()
                    .and_then(|stored| {
                        MediaSource::parse(
                            stored,
                            self.media_source.clone(),
                            self.media_raw.clone(),
                        )
                    })
                    .ok_or_else(|| {
                        StorageError::Custom(
                            "the transcript stores a media row with no source".to_owned(),
                        )
                    })?,
                text: self.media_text.clone(),
                media_type: self.media_type.clone(),
                detail: self.media_detail.clone(),
            },
            "reasoning" => TranscriptRow::Reasoning {
                text: self.content.clone(),
                redacted: self.reasoning_redacted.unwrap_or_default() != 0,
            },
            "opaque" => TranscriptRow::Opaque {
                item: self.opaque_item.clone().ok_or_else(|| {
                    StorageError::Custom(
                        "the transcript stores an opaque row with no item".to_owned(),
                    )
                })?,
                replay: self.opaque_replay.unwrap_or_default() != 0,
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

// endregion: --- Support
