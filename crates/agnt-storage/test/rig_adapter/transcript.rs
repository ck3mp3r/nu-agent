use super::*;

use crate::{
    Entries, MediaKind, MediaOwner, MediaSource, ToolCallStatus, TranscriptRecord, TranscriptRow,
    TranscriptStamp, Work,
};

/// The store hands out the rows it folded, in the order the fold gave them.
#[tokio::test]
async fn test_rig_adapter_transcript_returns_the_folded_rows_in_position_order() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-transcript")?;
    let mut rows = the_transcript();
    rows.reverse();

    // -- Exec
    Entries::new(db).record_transcript(rows).await?;
    let read = store.transcript()?;

    // -- Check
    let expected = the_transcript();
    assert_eq!(read, expected, "the rows SHALL come back in fold order");
    Ok(())
}

/// The same rows folded into two stores derive equal ids: a persisted row is
/// the row it replays, in any process.
#[tokio::test]
async fn test_rig_adapter_transcript_is_one_fold_in_two_stores() -> Result<()> {
    // -- Setup & Fixtures
    let (first_db, first) = memory_database("session-transcript")?;
    let (second_db, second) = memory_database("session-transcript")?;

    // -- Exec
    Entries::new(first_db)
        .record_transcript(the_transcript())
        .await?;
    Entries::new(second_db)
        .record_transcript(the_transcript())
        .await?;
    let ids = |records: Vec<TranscriptRecord>| -> Vec<String> {
        records
            .iter()
            .map(|record| record.id().as_str().to_owned())
            .collect()
    };

    // -- Check
    let left = ids(first.transcript()?);
    assert_eq!(
        left,
        ids(second.transcript()?),
        "one fold SHALL derive one set of ids"
    );
    assert!(!left.is_empty(), "the transcript SHALL hold rows");
    Ok(())
}

/// A row written twice at one position keeps one row, carrying the new content
/// and the id that content derives.
#[tokio::test]
async fn test_rig_adapter_transcript_replaces_the_row_at_one_position() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-transcript")?;
    let entries = Entries::new(db);
    entries
        .record_transcript(vec![record(
            0,
            TranscriptRow::Assistant {
                content: "first answer".to_owned(),
            },
        )])
        .await?;
    let rewritten = record(
        0,
        TranscriptRow::Assistant {
            content: "second answer".to_owned(),
        },
    );

    // -- Exec
    entries.record_transcript(vec![rewritten.clone()]).await?;
    let read = store.transcript()?;

    // -- Check
    assert_eq!(read, vec![rewritten], "one position SHALL hold one row");
    Ok(())
}

/// A row whose stored id is not the id its content derives is refused, not
/// handed out: an id that disagrees with its content is the defect.
#[tokio::test]
async fn test_rig_adapter_transcript_refuses_an_id_its_content_does_not_derive() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-transcript")?;
    Entries::new(Arc::clone(&db))
        .record_transcript(vec![record(
            0,
            TranscriptRow::System {
                content: "one system row".to_owned(),
            },
        )])
        .await?;
    db.call(Tamper).await??;

    // -- Exec
    let refusal = store
        .transcript()
        .err()
        .ok_or("a tampered id SHALL fail the read")?;

    // -- Check
    assert!(
        refusal
            .to_string()
            .contains("not the id its content derives"),
        "the refusal SHALL say what disagrees: {refusal}"
    );
    Ok(())
}

/// A session reads its own transcript and never another session's.
#[tokio::test]
async fn test_rig_adapter_transcript_is_scoped_to_its_session() -> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-mine")?;
    let entries = Entries::new(Arc::clone(&db));
    entries
        .record_transcript(vec![
            TranscriptRecord::new(
                "session-mine",
                0,
                TranscriptRow::Assistant {
                    content: "mine".to_owned(),
                },
            ),
            TranscriptRecord::new(
                "session-other",
                0,
                TranscriptRow::Assistant {
                    content: "theirs".to_owned(),
                },
            ),
        ])
        .await?;

    // -- Exec
    let read = store.transcript()?;

    // -- Check
    assert_eq!(read.len(), 1, "one session's rows only: {read:?}");
    assert_eq!(read[0].session(), "session-mine");
    Ok(())
}

/// A fold that keeps fewer blocks than the transcript holds renumbers from the
/// first block it keeps: no position past the new end survives.
#[tokio::test]
async fn test_rig_adapter_transcript_removes_the_positions_a_shrunk_fold_no_longer_reaches()
-> Result<()> {
    // -- Setup & Fixtures
    let (db, store) = memory_database("session-transcript")?;
    let entries = Entries::new(db);
    entries.record_transcript(the_transcript()).await?;
    let shrunk = vec![
        record(
            0,
            TranscriptRow::Compaction {
                content: "the summary of what was swept".to_owned(),
            },
        ),
        record(
            1,
            TranscriptRow::Assistant {
                content: "what the model answered after the fold".to_owned(),
            },
        ),
    ];

    // -- Exec
    entries.record_transcript(shrunk.clone()).await?;
    let read = store.transcript()?;

    // -- Check
    assert_eq!(
        read, shrunk,
        "the folded transcript SHALL hold the blocks the fold kept, and no more"
    );
    Ok(())
}

// region:    --- Support

/// The row at `row_seq` of the fixture session.
fn record(row_seq: i64, row: TranscriptRow) -> TranscriptRecord {
    TranscriptRecord::new("session-transcript", row_seq, row)
}

/// One transcript of every kind.
fn the_transcript() -> Vec<TranscriptRecord> {
    vec![
        record(
            0,
            TranscriptRow::User {
                content: "what the user asked".to_owned(),
                stamp: Some(TranscriptStamp {
                    millis: 1_790_974_800_213,
                    model: Some("anthropic/claude-sonnet-4-5".to_owned()),
                    agent: Some("architect".to_owned()),
                }),
            },
        )
        .with_turn_seq(Some(1)),
        record(
            1,
            TranscriptRow::Assistant {
                content: "what the model answered".to_owned(),
            },
        )
        .with_turn_seq(Some(1)),
        record(
            2,
            TranscriptRow::Tool {
                call_id: "call-read".to_owned(),
                name: "read".to_owned(),
                executed_name: Some("read_file".to_owned()),
                arguments: Some("{\"path\":\"a.txt\"}".to_owned()),
                result: Some("the file's text".to_owned()),
                success: Some(true),
                status: Some(ToolCallStatus::Success),
            },
        )
        .with_turn_seq(Some(1)),
        record(
            3,
            TranscriptRow::Tool {
                call_id: "call-write".to_owned(),
                name: "write".to_owned(),
                executed_name: None,
                arguments: Some("{\"path\":\"b.txt\"}".to_owned()),
                result: None,
                success: None,
                status: Some(ToolCallStatus::Cancelled),
            },
        )
        .with_turn_seq(Some(2)),
        record(
            4,
            TranscriptRow::Compaction {
                content: "the summary".to_owned(),
            },
        ),
        record(
            5,
            TranscriptRow::System {
                content: "a system message".to_owned(),
            },
        )
        .with_turn_seq(Some(2)),
        record(
            6,
            TranscriptRow::Media {
                owner: MediaOwner::User,
                kind: MediaKind::Document,
                source: MediaSource::Base64("JVBERi0xLjQ=".to_owned()),
                text: None,
                media_type: Some("pdf".to_owned()),
                detail: None,
            },
        ),
        record(
            7,
            TranscriptRow::Media {
                owner: MediaOwner::ToolResult,
                kind: MediaKind::Image,
                source: MediaSource::Raw(vec![0x89, b'P', b'N', b'G']),
                text: None,
                media_type: Some("png".to_owned()),
                detail: Some("high".to_owned()),
            },
        )
        .with_turn_seq(Some(2)),
        record(
            8,
            TranscriptRow::Reasoning {
                text: "the model's reasoning".to_owned(),
                redacted: false,
            },
        )
        .with_turn_seq(Some(2)),
        record(
            9,
            TranscriptRow::Opaque {
                item: "{\"type\":\"server_tool_use\"}".to_owned(),
                replay: true,
            },
        )
        .with_turn_seq(Some(2)),
    ]
}

/// Points every stored id at one value its content does not derive.
struct Tamper;

impl Work for Tamper {
    type Output = sqlx::Result<()>;

    async fn run(self, pool: sqlx::SqlitePool) -> Self::Output {
        sqlx::query("UPDATE transcript SET id = 'transcript-0000000000000000'")
            .execute(&pool)
            .await?;
        Ok(())
    }
}

// endregion: --- Support
