//!
//! The shadow record: the attach path folds a session's log into entries and
//! writes them as the store's transcript rows.

use super::*;

use crate::hook::TurnToolRecord;
use crate::session::RigStoreConfig;
use crate::session::StoreType;
use crate::session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver};
use crate::session::settle::{FactStore, SettleInput};
use agnt_storage::{RigAdapter, ToolCallStatus, TranscriptRecord, TranscriptRow, TranscriptStamp};

/// The fold a session's log produces and the rows it writes are one transcript:
/// an attached session with user text, an answer and a settled call reads back
/// one row per block, in fold order, with the facts each row can carry.
#[tokio::test]
async fn attach_records_the_fold_as_the_session_transcript() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let cwd = std::path::PathBuf::from("/home/user/project");
    let session_id = format!("{}-shadow-write", crate::session::prefix::dir_prefix(&cwd));
    let call = "call-1";
    let keys = seed(&rig, &session_id).await?;
    let records = [TurnToolRecord {
        call_id: CallId::from_wire(call),
        name: ToolName::new("read_file")?,
        arguments: "{\"path\":\"/tmp/test.txt\"}".to_string(),
        result: Some("the file's text".to_string()),
        success: Some(true),
        error_kind: None,
        status: Some(ToolStatus::Success),
    }];
    FactStore::open(&rig)
        .await?
        .settle(SettleInput {
            session_id: &session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_800_000,
            keys: Some(keys[..2].to_vec()),
            records: &records,
        })
        .await?;

    // -- Exec: the attach path hydrates the session, so the fold writes its rows
    let resolved = DefaultSessionResolver::new(rig.clone())
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some("shadow-write".to_string()),
            cwd,
        })
        .await?;
    let read = RigAdapter::new(rig.facts_blocking()?, &session_id).transcript()?;

    // -- Check
    assert!(
        resolved.should_hydrate_transcript,
        "the attach SHALL hydrate the session it writes"
    );
    let expected = vec![
        TranscriptRecord::new(
            &session_id,
            0,
            TranscriptRow::User {
                content: "q1".to_string(),
                stamp: Some(TranscriptStamp {
                    millis: 1_790_974_800_000,
                    model: Some("anthropic/claude-sonnet-4-5".to_string()),
                    agent: Some("tester".to_string()),
                }),
            },
        ),
        TranscriptRecord::new(
            &session_id,
            1,
            TranscriptRow::Assistant {
                content: "a1".to_string(),
            },
        ),
        TranscriptRecord::new(
            &session_id,
            2,
            TranscriptRow::Tool {
                name: "read_file".to_string(),
                arguments: Some("{\"path\":\"/tmp/test.txt\"}".to_string()),
                result: Some("the file's text".to_string()),
                success: Some(true),
                status: Some(ToolCallStatus::Success),
            },
        ),
    ];
    assert_eq!(
        read, expected,
        "the fold's rows SHALL read back in fold order"
    );
    Ok(())
}

/// Attaching one session twice writes one transcript: the second fold replaces
/// each position instead of appending to it.
#[tokio::test]
async fn attach_twice_records_one_transcript() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let cwd = std::path::PathBuf::from("/home/user/project");
    let session_id = format!("{}-shadow-twice", crate::session::prefix::dir_prefix(&cwd));
    seed(&rig, &session_id).await?;
    let resolver = DefaultSessionResolver::new(rig.clone());
    let input = || SessionResolutionInput {
        use_tui: true,
        session_id: Some("shadow-twice".to_string()),
        cwd: cwd.clone(),
    };

    // -- Exec
    resolver.resolve(input()).await?;
    resolver.resolve(input()).await?;
    let read = RigAdapter::new(rig.facts_blocking()?, &session_id).transcript()?;

    // -- Check: three folded blocks, not six
    assert_eq!(read.len(), 3, "one position SHALL hold one row: {read:?}");
    assert_eq!(read[0].row_seq(), 0);
    assert_eq!(read[2].row_seq(), 2);
    Ok(())
}

// region:    --- Support

/// Seed the log of `session_id`: a prompt, an answer beside one settled call,
/// and the call's result. Answers rig's key for each entry it persisted.
async fn seed(rig: &RigStoreConfig, session_id: &str) -> Result<Vec<agnt_storage::MessageKey>> {
    let store = rig.open(session_id).await?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.extend(vec![
        Message::user("q1"),
        Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Text(Text::new("a1")),
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call-1"),
                ToolFunction::new(
                    ToolName::new("read_file")?,
                    json!({"path": "/tmp/test.txt"}),
                ),
            )),
        ])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call-1"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::Text(Text::new("the file's text"))],
                is_error: false,
            })],
        },
    ]);
    rig_memory::save(&*store, session_id, &mut conversation)?;
    let saved = rig_memory::load(&*store, session_id)?;
    Ok(crate::session::settle::entry_keys(saved.entries()))
}

// endregion: --- Support
