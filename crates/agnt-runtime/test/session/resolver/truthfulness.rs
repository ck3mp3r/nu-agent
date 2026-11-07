//!
//! The truthfulness oracle: the transcript a reload hydrates through the store's
//! rows is the transcript the live fold produces from the same log — block for
//! block, stamp for stamp, status for status.

use super::*;
use crate::hook::TurnToolRecord;
use crate::session::RigStoreConfig;
use crate::session::StoreType;
use crate::session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver};
use crate::session::settle::{FactStore, SettleInput};
use crate::types::{Image, Opaque};
use agnt_storage::{MessageKey, RigAdapter, TranscriptRow};

/// The summary rig-memory splices into the log when a compaction sweeps.
const SUMMARY: &str = "The conversation before this point was compacted into this summary:\n\n\
<summary>\nrolled up\n</summary>";

/// WHEN a session's log carries every block kind, THE re-hydration read SHALL
/// produce the transcript the live fold produces from the same log.
async fn live_transcript(
    rig: &RigStoreConfig,
    session_id: &str,
    cwd: &std::path::Path,
) -> Result<Vec<UiMessageSnapshot>> {
    let facts = FactStore::open(rig).await?.read(session_id).await?;
    let store = rig.open(session_id).await?;
    let conversation = rig_memory::load(&*store, session_id)?;
    let turns = facts.entry_turns();
    let entries: Vec<StoredEntry> = conversation
        .entries()
        .map(|entry| {
            let key = crate::session::settle::entry_keys([entry]);
            StoredEntry {
                message: entry.message.clone(),
                author: entry.author.cloned(),
                turn_seq: key.first().and_then(|key| turns.get(key)).copied(),
            }
        })
        .collect();
    Ok(crate::session::resolver::hydrate_transcript_messages(
        &entries,
        cwd,
        &facts.status_map(),
        &facts.turns,
    ))
}

#[tokio::test]
async fn attach_hydrates_the_transcript_the_live_fold_produces() -> Result<()> {
    // -- Setup & Fixtures: a log carrying every block kind the record keeps
    let temp_dir = tempfile::tempdir()?;
    let rig = RigStoreConfig::new(StoreType::Sqlite).with_cache_dir(temp_dir.path().join("cache"));
    let cwd = std::path::PathBuf::from("/home/user/project");
    let session_id = format!("{}-truth", crate::session::prefix::dir_prefix(&cwd));
    let keys = seed(&rig, &session_id).await?;
    let records = [TurnToolRecord {
        call_id: CallId::from_wire("call-2"),
        name: ToolName::new("nu")?,
        arguments: "{\"command\":\"ls\"}".to_string(),
        result: None,
        success: None,
        error_kind: None,
        status: Some(ToolStatus::Cancelled),
    }];
    FactStore::open(&rig)
        .await?
        .settle(SettleInput {
            session_id: &session_id,
            model: "anthropic/claude-sonnet-4-5",
            agent: "tester",
            submitted_millis: 1_790_974_800_000,
            keys: Some(keys),
            records: &records,
        })
        .await?;

    // -- Exec: the attach path hydrates through the store's rows
    let resolved = DefaultSessionResolver::new(rig.clone())
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some("truth".to_string()),
            cwd: cwd.clone(),
        })
        .await?;
    let expected = live_transcript(&rig, &session_id, &cwd).await?;

    // -- Check: the two transcripts are one, and the record holds every carrier
    assert!(
        resolved.should_hydrate_transcript,
        "the attach SHALL hydrate the session it reads"
    );
    assert_eq!(
        resolved.initial_messages, expected,
        "the store's rows SHALL hydrate the transcript the live fold produces"
    );
    let rows = RigAdapter::new(rig.facts_blocking()?, &session_id).transcript()?;
    for kind in [
        "user",
        "assistant",
        "tool",
        "compaction",
        "media",
        "reasoning",
        "opaque",
    ] {
        assert!(
            rows.iter().any(|record| kind_of(record.row()) == kind),
            "the record SHALL keep every {kind} block: {rows:?}"
        );
    }
    Ok(())
}

// region:    --- Support

/// The kind token of `row`.
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

/// Seed `session_id`'s log with every block kind, each answered where it asks,
/// and answer the keys rig gave the entries.
async fn seed(rig: &RigStoreConfig, session_id: &str) -> Result<Vec<MessageKey>> {
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
            AssistantContent::Text(Text::new("")),
        ])),
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("call-1"),
                name: ToolName::new("read_file")?,
                content: vec![ToolResultContent::Text(Text::new("the file's text"))],
                is_error: false,
            })],
        },
        Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::Reasoning(rig::completion::message::Reasoning::new("thinking")),
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire("call-2"),
                ToolFunction::new(ToolName::new("nu")?, json!({"command": "ls"})),
            )),
            AssistantContent::Image(Image {
                data: crate::types::DocumentSourceKind::Base64("iVBORw0KGgo=".to_string()),
                ..Image::default()
            }),
            AssistantContent::Opaque(Opaque {
                item: json!({"type": "server_tool_use"}),
                replay: true,
            }),
        ])),
        Message::user("q2"),
        Message::System {
            content: "a system note".to_string(),
        },
    ]);
    // The compaction's summary is an entry the store authored, not the user's.
    conversation.push(
        rig::conversation::Entry::new(Message::user(SUMMARY)).by(rig_memory::summary_author()),
    );
    rig_memory::save(&*store, session_id, &mut conversation)?;
    let saved = rig_memory::load(&*store, session_id)?;
    Ok(crate::session::settle::entry_keys(saved.entries()))
}

// endregion: --- Support
