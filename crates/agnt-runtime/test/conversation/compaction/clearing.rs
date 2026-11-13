//! The between-runs clearing splice: the outputs a clearing dropped are written
//! into the store once, and the slot that carries them hands them over once.
//!
//! The executor's between-runs slot takes the stashed clearing with `take_clear`,
//! so the same plan can never be persisted twice in one turn. These tests pin
//! both halves: the slot hands out the clearance exactly once, and one `land`
//! writes the placeholder and the notes the plan names.

use std::sync::Arc;

use rig_memory::{ClearToolOutputs, Store, summary_author};

use crate::bus::Bus;
use crate::conversation::compaction::cleared::land;
use crate::hook::chain::PendingSummary;
use crate::session::RigStoreConfig;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
    ToolResult, ToolResultContent, UserContent,
};
use agnt_kernel::bus::LifecycleEvent;

const CONV: &str = "conv-1";
const NOTES: &str = "the file listed three crates";

// region:    --- Test Support

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// An in-memory rig store: a memory store resolves no path, so no ambient cache
/// directory is touched.
async fn memory_store() -> Result<Arc<dyn Store>> {
    Ok(RigStoreConfig::in_memory().open(CONV).await?)
}

/// The policy whose clearing takes every output above one token.
fn clearing_policy() -> rig_memory::CompactionPolicy {
    rig_memory::CompactionPolicy {
        clearing: ClearToolOutputs {
            keep_tokens: 0,
            keep_under: 1,
        },
        ..rig_memory::CompactionPolicy::default()
    }
}

/// Seed the store with a `read` call and the output answering it, as a turn
/// leaves them.
async fn seeded(store: &Arc<dyn Store>) -> Result<()> {
    let call = ToolCall::new(
        CallId::from_wire("tc-1"),
        ToolFunction::new(
            ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
            serde_json::json!({ "path": "/tmp/whatever" }),
        ),
    );
    let result = ToolResult {
        call: CallId::from_wire("tc-1"),
        name: ToolName::new("read").map_err(|_| "read is a non-empty tool name")?,
        content: vec![ToolResultContent::text("o".repeat(4_000))],
        is_error: false,
    };
    let mut conversation = rig_memory::load(&**store, CONV)?;
    conversation.extend([
        Message::user("the turn's prompt"),
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            call,
        )])),
        Message::User {
            content: vec![UserContent::ToolResult(result)],
        },
        Message::assistant("the run's answer"),
    ]);
    rig_memory::save(&**store, CONV, &mut conversation)?;
    Ok(())
}

/// The tool-result texts of `message`, in order.
fn result_texts(message: &Message) -> Vec<String> {
    let Message::User { content } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(
                result
                    .content
                    .iter()
                    .filter_map(|content| content.as_text().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// The user text parts of `message`, in order.
fn user_texts(message: &Message) -> Vec<String> {
    let Message::User { content } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support

/// WHEN the executor lands a stashed clearing, THE splice SHALL write the
/// placeholder over the output the plan names, embed its notes once, and
/// announce one `Completed` row — and SHALL leave every other message alone.
#[tokio::test]
async fn land_writes_the_placeholder_and_the_notes_once() -> Result<()> {
    // -- Setup & Fixtures: a stored tool call and its output, and the plan the
    // decision stashed for it.
    let store = memory_store().await?;
    seeded(&store).await?;
    let before = rig_memory::load(&*store, CONV)?.messages().len();
    let policy = clearing_policy();
    let conversation = rig_memory::load(&*store, CONV)?;
    let plan = policy.clearing.plan(&conversation);
    assert_eq!(
        plan.cleared.results, 1,
        "the plan must clear the one output"
    );
    let pending = PendingSummary::default();
    pending.stash_clear(plan, Some(NOTES.to_string()));
    let clearance = pending
        .take_clear()
        .ok_or("the stashed clearing must reach the executor")?;
    let bus = Bus::default();
    let mut rx = bus.lifecycle().subscribe();

    // -- Exec
    let landed = land(
        clearance,
        Arc::clone(&store),
        CONV.to_string(),
        bus.stream(),
        bus.lifecycle(),
    )
    .await;

    // -- Check: one placeholder, one notes part, one Completed row, and the same
    // number of messages.
    assert!(landed, "the clearing must land in the store");
    let conversation = rig_memory::load(&*store, CONV)?;
    assert_eq!(
        conversation.messages().len(),
        before,
        "a clearing rewrites outputs; it removes no message"
    );
    let placeholders = conversation
        .messages()
        .iter()
        .flat_map(|message| result_texts(message))
        .filter(|text| text == ClearToolOutputs::PLACEHOLDER)
        .count();
    assert_eq!(
        placeholders, 1,
        "the cleared output must read as the placeholder exactly once"
    );
    let notes = conversation
        .messages()
        .iter()
        .flat_map(|message| user_texts(message))
        .filter(|text| text.contains("<cleared-output-notes>"))
        .count();
    assert_eq!(notes, 1, "the notes must be embedded exactly once");
    assert!(
        conversation
            .messages()
            .iter()
            .any(|message| user_texts(message).iter().any(|text| text.contains(NOTES))),
        "the embedded notes must carry what the model wrote"
    );
    assert_eq!(
        conversation.entries().next().and_then(|entry| entry.author),
        None,
        "a clearing keeps every message: it inserts no summary entry"
    );
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive CompactionCompleted within timeout")?
        .map_err(|_| "should receive CompactionCompleted")?;
    assert!(
        matches!(&completed, LifecycleEvent::CompactionCompleted { summary, .. } if summary.is_empty()),
        "a clearing finishes with an empty summary, got {completed:?}"
    );
    let author = summary_author();
    assert!(
        conversation
            .entries()
            .all(|entry| entry.author != Some(&author)),
        "a clearing is not a summary: every entry keeps its author"
    );
    Ok(())
}

/// WHEN the executor's between-runs slot hands the clearing over, THE slot SHALL
/// hand it over exactly once: a second take finds nothing, so no plan is ever
/// persisted twice in one turn.
#[tokio::test]
async fn the_between_runs_slot_hands_the_clearing_over_once() -> Result<()> {
    // -- Setup & Fixtures
    let store = memory_store().await?;
    seeded(&store).await?;
    let policy = clearing_policy();
    let conversation = rig_memory::load(&*store, CONV)?;
    let plan = policy.clearing.plan(&conversation);
    let pending = PendingSummary::default();
    pending.stash_clear(plan, None);

    // -- Exec & Check: the first take hands it out, the second finds nothing.
    assert!(
        pending.take_clear().is_some(),
        "the slot must hand the stashed clearing to the executor"
    );
    assert!(
        pending.take_clear().is_none(),
        "the slot must hand the clearing over exactly once"
    );

    // -- Exec & Check: the turn-end sweep leaves no slot behind.
    let policy = clearing_policy();
    let conversation = rig_memory::load(&*store, CONV)?;
    pending.stash_clear(policy.clearing.plan(&conversation), None);
    pending.clear();
    assert!(
        pending.take_clear().is_none(),
        "the turn-end sweep must drop a clearing the turn never landed"
    );
    Ok(())
}
