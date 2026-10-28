//! Shared fixtures for the compactor test modules.
//!
//! Holds the imports, the `Result` alias, the store builders, and the helper
//! functions every topical compactor test file needs. Sub-files pull them in
//! with `use super::*;`.

pub(super) use crate::bus::{Bus, CompactionEvent, CompactionRx};
pub(super) use crate::conversation::compaction::compactor::{NuCompactor, resolve_model_spec};
pub(super) use crate::session::{MemoryError, RigStoreConfig, StoreType};
pub(super) use crate::types::{Message, UserContent};

pub(super) use rig::DynModel;
pub(super) use rig::catalog::ModelSpec;
pub(super) use rig::operation::Completion;
pub(super) use rig::test_utils::{MockCompletionModel, MockStreamEvent};
pub(super) use rig_memory::{CompactReason, CompactionPolicy, Store, summary_author};

pub(super) use std::sync::Arc;
pub(super) use tempfile::TempDir;

/// Test result alias per AGENTS.md: tests use `.ok_or("...")?`.
pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The conversation every test compacts.
pub(super) const CONV: &str = "conv-1";

/// The leading text of every compaction summary message, as rig-memory writes
/// it.
pub(super) const SUMMARY_PREFIX: &str =
    "The conversation before this point was compacted into this summary:";

// region:    --- Test Support

/// The spec the compactor plans against: a registered provider with a window
/// wide enough for the summarizer's default reserve and summary budget.
pub(super) fn test_spec() -> Result<ModelSpec> {
    resolve_model_spec("openai", "gpt-4o", Some(200_000), Some(1_024))
        .ok_or_else(|| "openai should resolve to a registered provider id".into())
}

/// A policy that keeps nothing recent, so a short conversation is still
/// summarized.
pub(super) fn test_policy() -> CompactionPolicy {
    CompactionPolicy {
        keep_recent: 0,
        keep_asked: 0,
        ..CompactionPolicy::default()
    }
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

/// Build a `NuCompactor` over a scripted streaming model.
pub(super) fn make_compactor(chunks: &[&str], bus: Bus) -> Result<NuCompactor> {
    Ok(NuCompactor::new(
        stream_model(chunks).erase(),
        bus,
        test_policy(),
        Some(test_spec()?),
    ))
}

/// An in-memory SQLite rig store.
///
/// A memory store resolves no path, so no ambient cache directory is touched.
pub(super) fn memory_store() -> Result<Arc<dyn Store>> {
    Ok(RigStoreConfig::in_memory().open(CONV)?)
}

/// A file-backed `DirStore` rig store rooted at `cache_dir`.
pub(super) fn dir_store(cache_dir: &std::path::Path) -> Result<Arc<dyn Store>> {
    Ok(RigStoreConfig::new(StoreType::Jsonl)
        .with_cache_dir(cache_dir)
        .open(CONV)?)
}

/// Seed `store` with a short conversation: two user turns and two replies.
pub(super) fn seed(store: &dyn Store) -> Result<()> {
    let mut conversation = rig_memory::load(store, CONV)?;
    conversation.extend([
        Message::user("first question"),
        Message::assistant("first answer"),
        Message::user("second question"),
        Message::assistant("second answer"),
    ]);
    rig_memory::save(store, CONV, &mut conversation)?;
    Ok(())
}

/// Extract the concatenated user text blocks from a `Message::User`.
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

/// Extract the last (prompt) message text from the most recent recorded request.
///
/// `MockCompletionModel` records every request it receives; the summarizer sends
/// its prompt as the last `chat_history` message. This lets the tests assert what
/// text the summarizer actually sent to the model.
pub(super) fn last_request_prompt(model: &MockCompletionModel) -> Result<String> {
    let requests = model.requests();
    let last = requests.last().ok_or("at least one request recorded")?;
    let prompt = last.chat_history.last().ok_or("chat_history non-empty")?;
    match &**prompt {
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

// endregion: --- Test Support
