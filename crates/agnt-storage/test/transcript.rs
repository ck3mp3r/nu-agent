//! The transcript record's identity: one id per content, derived from the row's
//! own field set.

use super::*;

use crate::ToolCallStatus;

// region:    --- Tests

/// Two records built from the same field set carry one id.
#[test]
fn test_transcript_id_is_equal_for_equal_content() -> Result<()> {
    // -- Setup & Fixtures
    let rows = every_row();

    // -- Exec & Check
    for row in rows {
        let left = TranscriptRecord::new("session-a", 4, row.clone());
        let right = TranscriptRecord::new("session-a", 4, row.clone());
        assert_eq!(
            left.id(),
            right.id(),
            "one field set SHALL fold to one id: {row:?}"
        );
    }
    Ok(())
}

/// A pinned id names the derivation itself: a persisted row re-derives the same
/// id in any later process and any later build, or every row it named is
/// orphaned. Changing the encoding changes this test.
#[test]
fn test_transcript_id_is_pinned() -> Result<()> {
    // -- Setup & Fixtures
    let record = TranscriptRecord::new(
        "session-a",
        2,
        TranscriptRow::Tool {
            call_id: "call-a".to_owned(),
            name: "read".to_owned(),
            executed_name: None,
            arguments: Some("{\"path\":\"a.txt\"}".to_owned()),
            result: Some("the file's text".to_owned()),
            success: Some(true),
            status: Some(ToolCallStatus::Success),
        },
    );

    // -- Exec
    let id = record.id();

    // -- Check
    assert_eq!(id.as_str(), "transcript-0bfae7cf18c7334e");
    Ok(())
}

/// Every field of the row set moves the id: a content edit, another position,
/// another session, another kind, and another tool disposition.
#[test]
fn test_transcript_id_follows_the_row_field_set() -> Result<()> {
    // -- Setup & Fixtures
    let base = TranscriptRecord::new(
        "session-a",
        1,
        TranscriptRow::User {
            content: "hello".to_owned(),
            stamp: Some(TranscriptStamp {
                millis: 1_790_974_800_213,
                model: Some("anthropic/claude-sonnet-4-5".to_owned()),
                agent: Some("architect".to_owned()),
            }),
        },
    );
    let moved = [
        TranscriptRecord::new(
            "session-a",
            1,
            TranscriptRow::User {
                content: "hello!".to_owned(),
                stamp: Some(stamp()),
            },
        ),
        TranscriptRecord::new(
            "session-a",
            1,
            TranscriptRow::User {
                content: "hello".to_owned(),
                stamp: None,
            },
        ),
        TranscriptRecord::new(
            "session-a",
            1,
            TranscriptRow::User {
                content: "hello".to_owned(),
                stamp: Some(stamp_with_agent(None)),
            },
        ),
        TranscriptRecord::new("session-a", 2, base.row().clone()),
        TranscriptRecord::new("session-b", 1, base.row().clone()),
        TranscriptRecord::new(
            "session-a",
            1,
            TranscriptRow::Assistant {
                content: "hello".to_owned(),
            },
        ),
        base.clone().with_turn_seq(Some(1)),
    ];

    // -- Exec & Check
    for record in moved {
        assert_ne!(
            base.id(),
            record.id(),
            "a moved field SHALL move the id: {record:?}"
        );
    }
    Ok(())
}

/// An absent tool part and an empty one are different rows, so they carry
/// different ids: the encoding tags presence, never emptiness.
#[test]
fn test_transcript_id_separates_absent_from_empty() -> Result<()> {
    // -- Setup & Fixtures
    let absent = tool_row(None, None, None, None);
    let empty = tool_row(Some(String::new()), None, None, None);

    // -- Exec & Check
    assert_ne!(absent.id(), empty.id());
    Ok(())
}

/// The post-repair name is a carrier of its own: a row that names the executed
/// tool is not the row that names none.
#[test]
fn test_transcript_id_separates_an_executed_name_from_none() -> Result<()> {
    // -- Setup & Fixtures
    let unnamed = tool_row(None, None, None, None);
    let named = TranscriptRecord::new(
        "session-a",
        3,
        TranscriptRow::Tool {
            call_id: "call-a".to_owned(),
            name: "read".to_owned(),
            executed_name: Some("read_file".to_owned()),
            arguments: None,
            result: None,
            success: None,
            status: None,
        },
    );

    // -- Exec & Check
    assert_ne!(unnamed.id(), named.id());
    Ok(())
}

/// A media block's bytes are part of the row: two images of one kind with
/// different bytes are two rows.
#[test]
fn test_transcript_id_carries_the_media_source() -> Result<()> {
    // -- Setup & Fixtures
    let row = |bytes: Vec<u8>| {
        TranscriptRecord::new(
            "session-a",
            4,
            TranscriptRow::Media {
                owner: MediaOwner::Assistant,
                kind: MediaKind::Image,
                source: MediaSource::Raw(bytes),
                text: None,
                media_type: Some("png".to_owned()),
                detail: None,
            },
        )
    };
    let left = row(vec![1, 2, 3]);
    let right = row(vec![1, 2, 3, 4]);

    // -- Exec & Check
    assert_ne!(left.id(), right.id());
    Ok(())
}

// endregion: --- Tests

// region:    --- Support

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The stamp every user row carries, unless a test moves one field of it.
fn stamp() -> TranscriptStamp {
    TranscriptStamp {
        millis: 1_790_974_800_213,
        model: Some("anthropic/claude-sonnet-4-5".to_owned()),
        agent: Some("architect".to_owned()),
    }
}

/// The stamp with another persona.
fn stamp_with_agent(agent: Option<&str>) -> TranscriptStamp {
    TranscriptStamp {
        agent: agent.map(str::to_owned),
        ..stamp()
    }
}

/// One tool row, with the parts a test names.
fn tool_row(
    arguments: Option<String>,
    result: Option<String>,
    success: Option<bool>,
    status: Option<ToolCallStatus>,
) -> TranscriptRecord {
    TranscriptRecord::new(
        "session-a",
        3,
        TranscriptRow::Tool {
            call_id: "call-a".to_owned(),
            name: "read".to_owned(),
            executed_name: None,
            arguments,
            result,
            success,
            status,
        },
    )
}

/// One row of every kind.
fn every_row() -> Vec<TranscriptRow> {
    vec![
        TranscriptRow::User {
            content: "hello".to_owned(),
            stamp: Some(stamp()),
        },
        TranscriptRow::User {
            content: "hello".to_owned(),
            stamp: None,
        },
        TranscriptRow::Assistant {
            content: "hi".to_owned(),
        },
        TranscriptRow::System {
            content: "the system's words".to_owned(),
        },
        TranscriptRow::Tool {
            call_id: "call-b".to_owned(),
            name: "read".to_owned(),
            executed_name: Some("read_file".to_owned()),
            arguments: Some("{}".to_owned()),
            result: None,
            success: Some(false),
            status: Some(ToolCallStatus::Cancelled),
        },
        TranscriptRow::Compaction {
            content: "the summary".to_owned(),
        },
        TranscriptRow::Media {
            owner: MediaOwner::User,
            kind: MediaKind::Document,
            source: MediaSource::Url("file:///tmp/a.pdf".to_owned()),
            text: Some("the document's own text".to_owned()),
            media_type: Some("pdf".to_owned()),
            detail: None,
        },
        TranscriptRow::Media {
            owner: MediaOwner::ToolResult,
            kind: MediaKind::Image,
            source: MediaSource::Raw(vec![0x89, b'P', b'N', b'G']),
            text: None,
            media_type: Some("png".to_owned()),
            detail: Some("high".to_owned()),
        },
        TranscriptRow::Reasoning {
            text: "the model's reasoning".to_owned(),
            redacted: false,
        },
        TranscriptRow::Opaque {
            item: "{\"type\":\"server_tool_use\"}".to_owned(),
            replay: true,
        },
    ]
}

// endregion: --- Support
