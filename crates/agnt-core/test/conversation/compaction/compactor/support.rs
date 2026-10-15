//! Shared fixtures for the compactor test modules.
//!
//! Holds the imports, the `Result` alias, the artifact constants, and the
//! helper functions every topical compactor test file needs. Sub-files pull
//! them in with `use super::*;`.

pub(super) use crate::bus::{Bus, CompactionEvent, CompactionRx};
pub(super) use crate::conversation::compaction::compactor::{NoopStore, NuCompactor};
pub(super) use crate::session::{CompactionMarker, FsSessionStore, SessionStore, StoreEntry};
pub(super) use crate::types::{Message, UserContent};
pub(super) use chrono::Utc;

pub(super) use rig::DynModel;
pub(super) use rig::memory::MemoryError;
pub(super) use rig::operation::Completion;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};

pub(super) use std::sync::Arc;
pub(super) use tempfile::TempDir;

/// Test result alias per AGENTS.md: tests use `.ok_or("...")?`.
pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The header every artifact body must start with.
pub(super) const SUMMARY_HEADER: &str = "What we did thus far:\n\n";
/// Marker appended when the artifact body is truncated to `max_bytes`.
pub(super) const TRUNCATION_MARKER: &str = "[…truncated…]";

// region:    --- Test Support

/// Extract the concatenated user text blocks from a `Message::User`.
///
/// Tests inspect the artifact through the public `Into<Message>` boundary
/// rather than reaching into compactor internals.
pub(super) fn user_text(msg: &Message) -> String {
    let Message::User { content } = msg else {
        panic!("expected Message::User, got {msg:?}");
    };
    content
        .iter()
        .filter_map(|c| match c {
            UserContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a `MockCompletionModel` that streams `chunks` then terminates.
pub(super) fn stream_model(chunks: &[&str]) -> MockCompletionModel {
    let mut turn = chunks
        .iter()
        .map(|c| MockStreamEvent::Text(c.to_string()))
        .collect::<Vec<_>>();
    turn.push(MockStreamEvent::final_response_with_default_usage());
    MockCompletionModel::from_stream_turns([turn])
}

/// Extract the last (prompt) message text from the most recent recorded request.
///
/// `MockCompletionModel` records every request it receives; the builder appends
/// the prompt as the last `chat_history` message. This lets the tests assert
/// what text the summarizer actually sent to the model.
pub(super) fn last_request_prompt(model: &MockCompletionModel) -> Result<String> {
    let requests = model.requests();
    let last = requests.last().ok_or("at least one request recorded")?;
    let prompt = last.chat_history.last().ok_or("chat_history non-empty")?;
    match prompt {
        Message::User { content } => Ok(content
            .iter()
            .filter_map(|c| match c {
                UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")),
        Message::System { content } => Ok(content.clone()),
        Message::Assistant(assistant) => Ok(assistant
            .content
            .iter()
            .filter_map(|c| match c {
                crate::types::AssistantContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")),
    }
}

pub(super) fn make_compactor(
    model: MockCompletionModel,
    bus: Bus,
    max_bytes: Option<usize>,
) -> NuCompactor {
    NuCompactor::new(model.erase(), bus, max_bytes)
}

/// Build a `NuCompactor` attached to a backing store of type `S`.
pub(super) fn make_compactor_with_store<S: SessionStore + Clone + Send + Sync>(
    model: MockCompletionModel,
    bus: Bus,
    max_bytes: Option<usize>,
    store: Arc<S>,
) -> NuCompactor<S> {
    NuCompactor::new(model.erase(), bus, max_bytes).with_store(store)
}

/// Drain compaction bus events until a `CompactionEvent::Failed` is received,
/// returning its message. Asserts the stream terminates with a `Failed`.
pub(super) async fn recv_until_failed(rx: &mut CompactionRx) -> Result<String> {
    loop {
        let event = rx
            .recv()
            .await
            .map_err(|_| "should receive a Failed event")?;
        if let CompactionEvent::Failed { message, .. } = event {
            return Ok(message);
        }
    }
}

/// Build an `FsSessionStore` seeded with a `CompactionMarker` for `conv`.
/// Returns the store, a shared `Arc` to attach to the compactor, the marker
/// summary, and the `TempDir` that keeps the backing files alive for the
/// duration of the test.
pub(super) async fn seeded_marker_store(
    conv: &str,
    summary: &str,
) -> Result<(FsSessionStore, Arc<FsSessionStore>, String, TempDir)> {
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let marker = CompactionMarker::new(summary.to_string(), Utc::now());
    store
        .append(conv, &[StoreEntry::Marker(marker)])
        .await
        .map_err(|_| "should append marker")?;
    let summary = summary.to_string();
    let arc = Arc::new(store.clone());
    Ok((store, arc, summary, temp_dir))
}

// endregion: --- Test Support
