//! Tests for the `NuCompactor` rig `Compactor` adapter.
//!
//! `NuCompactor` (implemented in `compactor.rs`, created by the implementation
//! task) wraps the sliding-summary summarizer so it satisfies the rig 0.42.0
//! `Compactor` trait. These tests pin the observable behavior through the
//! public boundary: the produced `Artifact`, the summarizer LLM prompt input,
//! the bus events emitted during streaming, the optional `max_bytes` cap, and
//! the error mapping.

use crate::bus::{Bus, CompactionEvent};
use crate::conversation::compaction::compactor::{NoopStore, NuCompactor};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::{CompactionMarker, FsSessionStore, SessionStore, StoreEntry};
use crate::types::{Message, UserContent};
use chrono::Utc;

use rig::agent::ModelHandle;
use rig::memory::MemoryError;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use std::sync::Arc;
use tempfile::TempDir;

/// The header every artifact body must start with.
const SUMMARY_HEADER: &str = "What we did thus far:\n\n";
/// Marker appended when the artifact body is truncated to `max_bytes`.
const TRUNCATION_MARKER: &str = "[…truncated…]";

/// A `ProgressUi` that never requests cancellation and ignores tick events.
struct TestProgressUi;

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, _event: &UiEvent) {}
    fn flush(&mut self) {}
    fn take_cancel_requested(&self) -> bool {
        false
    }
}

/// Extract the concatenated user text blocks from a `Message::User`.
///
/// Tests inspect the artifact through the public `Into<Message>` boundary
/// rather than reaching into compactor internals.
fn user_text(msg: &Message) -> String {
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
fn stream_model(chunks: &[&str]) -> MockCompletionModel {
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
fn last_request_prompt(model: &MockCompletionModel) -> String {
    let requests = model.requests();
    let last = requests.last().expect("at least one request recorded");
    let prompt = last.chat_history.last().expect("chat_history non-empty");
    match prompt {
        Message::User { content } => content
            .iter()
            .filter_map(|c| match c {
                UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
        Message::System { content } => content.clone(),
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                crate::types::AssistantContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn make_compactor(model: MockCompletionModel, bus: Bus, max_bytes: Option<usize>) -> NuCompactor {
    NuCompactor::new(ModelHandle::new(model), TestProgressUi, bus, max_bytes)
}

/// Build a `NuCompactor` attached to a backing store of type `S`.
fn make_compactor_with_store<S: SessionStore + Clone + Send + Sync>(
    model: MockCompletionModel,
    bus: Bus,
    max_bytes: Option<usize>,
    store: Arc<S>,
) -> NuCompactor<S> {
    NuCompactor::new(ModelHandle::new(model), TestProgressUi, bus, max_bytes).with_store(store)
}

/// Drain compaction bus events until a `CompactionEvent::Failed` is received,
/// returning its message. Asserts the stream terminates with a `Failed`.
async fn recv_until_failed(
    rx: &mut tokio::sync::broadcast::Receiver<CompactionEvent>,
) -> Result<String> {
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

/// Test result alias per AGENTS.md: tests use `.ok_or("...")?`.
type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn artifact_is_user_message_with_required_header_and_summary() {
    let bus = Bus::new();
    let model = stream_model(&["decided to refactor the cache"]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .expect("compact should succeed");

    let msg: Message = artifact.into();
    assert!(
        matches!(msg, Message::User { .. }),
        "artifact must convert to Message::User, got {msg:?}"
    );
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}decided to refactor the cache")
    );
}

#[tokio::test]
async fn carry_over_summary_is_prepended_into_summarizer_input() {
    let bus = Bus::new();
    // Two scripted turns: the first produces the initial summary, the second
    // is compacted with that summary supplied as `carry_over`.
    let model = MockCompletionModel::from_stream_turns([
        [
            MockStreamEvent::Text("first summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        [
            MockStreamEvent::Text("second summary".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let compactor = make_compactor(model.clone(), bus, None);

    let evicted = vec![Message::user("second-batch of old messages")];
    let first_artifact = compactor
        .compact(
            "conv-1",
            &[Message::user("first-batch")],
            None,
            "test_source",
        )
        .await
        .expect("first compact should succeed");

    let _ = compactor
        .compact("conv-1", &evicted, Some(&first_artifact), "test_source")
        .await
        .expect("second compact should succeed");

    // The second (last) request's prompt must include the carry-over text.
    let prompt = last_request_prompt(&model);
    assert!(
        prompt.contains("first summary"),
        "carry-over summary missing from summarizer input, got: {prompt}"
    );
}

#[tokio::test]
async fn started_and_summary_chunk_events_are_emitted_on_bus() {
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["chunk one", "chunk two"]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .expect("compact should succeed");

    let started = rx.recv().await.expect("Started event emitted");
    assert!(
        matches!(started, CompactionEvent::Started { .. }),
        "first event should be Started, got {started:?}"
    );

    let chunk = rx.recv().await.expect("SummaryChunk event emitted");
    assert!(
        matches!(chunk, CompactionEvent::SummaryChunk { .. }),
        "expected a SummaryChunk after Started, got {chunk:?}"
    );
}

#[tokio::test]
async fn max_bytes_cap_truncates_body_at_utf8_boundary_preserving_header() {
    let bus = Bus::new();
    // A summary long enough that the full artifact far exceeds the cap.
    let summary = "alpha beta gamma delta epsilon zeta eta theta";
    let model = stream_model(&[summary]);
    let compactor = make_compactor(model, bus, Some(40));

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .expect("compact should succeed");

    let msg: Message = artifact.into();
    let text = user_text(&msg);
    let full = format!("{SUMMARY_HEADER}{summary}");

    assert!(
        full.len() > 40,
        "test precondition: full artifact must exceed the cap"
    );
    assert!(
        text.starts_with(SUMMARY_HEADER),
        "truncated artifact must keep the header, got: {text}"
    );
    assert!(
        text.ends_with(TRUNCATION_MARKER),
        "truncated artifact must end with the marker, got: {text}"
    );
    assert!(
        text.len() <= 40,
        "truncated artifact must not exceed max_bytes, got {text:?} ({} bytes)",
        text.len()
    );

    // The truncated body must be a UTF-8-boundary prefix of the full body,
    // so no grapheme is split mid-character.
    let body = text
        .strip_prefix(SUMMARY_HEADER)
        .and_then(|s| s.strip_suffix(TRUNCATION_MARKER))
        .expect("truncated text has header and marker");
    assert!(
        full.starts_with(&format!("{SUMMARY_HEADER}{body}")),
        "truncated body must be a prefix of the full body, got: {body}"
    );
}

#[tokio::test]
async fn summarizer_error_maps_to_memory_error_backend() {
    let bus = Bus::new();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::error("provider exploded"),
    ]]);
    let compactor = make_compactor(model, bus, None);

    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    assert!(
        matches!(result, Err(MemoryError::Backend(_))),
        "expected Err(MemoryError::Backend), got {result:?}"
    );
}

#[tokio::test]
async fn empty_summary_is_rejected_and_no_marker_written() -> Result<()> {
    // -- Setup & Fixtures
    // A stream that emits only reasoning (no text) leaves `aggregated` empty.
    // Compaction must reject it: emit Failed with "empty summary", return Err,
    // and NOT persist a marker.
    let (store, store_arc, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::reasoning("thinking hard"),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = make_compactor_with_store(model, bus, None, store_arc);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must reject an empty summary, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("empty summary"),
        "error must mention the empty summary, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("empty summary"),
        "expected Failed with empty summary, got {failed:?}"
    );
    // No Completed event and no new marker persisted.
    let (_, entries) = store.load("conv-1").await?.ok_or("should find session")?;
    let markers: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .collect();
    assert_eq!(
        markers.len(),
        1,
        "empty summary must NOT write a new marker, found {}",
        markers.len()
    );
    Ok(())
}

#[tokio::test]
async fn stream_error_surfaces_actual_error_message() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::error("provider exploded"),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must surface the stream error, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("provider exploded"),
        "error must carry the actual stream error message, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("provider exploded"),
        "Failed event must carry the actual stream error, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn tool_call_chunk_during_compaction_emits_failed() -> Result<()> {
    // -- Setup & Fixtures
    // No tools are registered during compaction, so a ToolCall chunk is
    // unexpected and must emit Failed rather than being swallowed.
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = make_compactor(model, bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let result = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await;

    // -- Check
    let Err(MemoryError::Backend(err)) = result else {
        panic!("compact must fail on a tool call chunk, got {result:?}");
    };
    let message = err.to_string();
    assert!(
        message.contains("tool call"),
        "error must mention the unexpected tool call, got: {message}"
    );
    let failed = recv_until_failed(&mut rx).await?;
    assert!(
        failed.contains("tool call"),
        "expected Failed mentioning tool call, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn set_model_swaps_the_model_used_by_the_next_compact_call() {
    let bus = Bus::new();
    let old_model = stream_model(&["old summary"]);
    let compactor = make_compactor(old_model, bus, None);

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new model summary"]);
    compactor.set_model(ModelHandle::new(new_model.clone()));

    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .expect("compact after set_model should succeed");

    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}new model summary"),
        "compact must use the model set via set_model()"
    );
    let prompt = last_request_prompt(&new_model);
    assert!(
        prompt.contains("old message"),
        "the swapped-in model received the evicted messages, got: {prompt}"
    );
}

#[tokio::test]
async fn from_shared_model_compactor_uses_model_swapped_on_the_shared_arc() {
    // -- Setup & Fixtures
    let bus = Bus::new();
    // The compactor is built from an external shared Arc; the old model is
    // scripted to produce a distinct summary that must NOT appear.
    let old_model = stream_model(&["old shared summary"]);
    let shared_arc: std::sync::Arc<std::sync::Mutex<ModelHandle>> =
        std::sync::Arc::new(std::sync::Mutex::new(ModelHandle::new(old_model)));

    let compactor: NuCompactor<NoopStore, TestProgressUi> =
        NuCompactor::from_shared_model(shared_arc.clone(), TestProgressUi, bus, None);

    // The new model is scripted to produce a distinct summary; keep a clone to
    // inspect the recorded request after the swap.
    let new_model = stream_model(&["new shared summary"]);
    *shared_arc.lock().expect("shared model mutex not poisoned") =
        ModelHandle::new(new_model.clone());

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .expect("compact after set_model on the shared Arc should succeed");

    // -- Check
    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}new shared summary"),
        "compact must use the model swapped via the shared Arc"
    );
    let prompt = last_request_prompt(&new_model);
    assert!(
        prompt.contains("old message"),
        "the swapped-in model received the evicted messages, got: {prompt}"
    );
}

// -- Test Support: seed a store with a single marker for a conversation.

/// Build an `FsSessionStore` seeded with a `CompactionMarker` for `conv`.
/// Returns the store, a shared `Arc` to attach to the compactor, the marker
/// summary, and the `TempDir` that keeps the backing files alive for the
/// duration of the test.
async fn seeded_marker_store(
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

#[tokio::test]
async fn marker_present_still_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists that already absorbed all the evicted messages. Compaction
    // must ALWAYS run the LLM — the marker summary is only used as carry-over,
    // never to skip the call.
    let (_, store, _marker_summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::new();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let msg: Message = artifact.into();
    assert_eq!(
        user_text(&msg),
        format!("{SUMMARY_HEADER}fresh LLM summary"),
        "compact must return the fresh LLM summary even when a marker exists"
    );
    assert_eq!(
        model.requests().len(),
        1,
        "compact must call the LLM even when a marker exists"
    );
    Ok(())
}

#[tokio::test]
async fn no_marker_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // No store attached -> no marker -> LLM must be called as before.
    let bus = Bus::new();
    let model = stream_model(&["fresh summary"]);
    let compactor = make_compactor(model.clone(), bus, None);

    // -- Exec
    let evicted = vec![Message::user("old message")];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        model.requests().len(),
        1,
        "no marker must trigger the LLM call"
    );
    let msg: Message = artifact.into();
    assert_eq!(user_text(&msg), format!("{SUMMARY_HEADER}fresh summary"));
    Ok(())
}

#[tokio::test]
async fn new_messages_beyond_marker_calls_llm() -> Result<()> {
    // -- Setup & Fixtures
    // Marker absorbed 2, but 3 are evicted now -> LLM must run.
    let (_, store, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "old cached summary").await?;
    let bus = Bus::new();
    let model = stream_model(&["new summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    let artifact = compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        model.requests().len(),
        1,
        "a marker must not skip the LLM call"
    );
    let msg: Message = artifact.into();
    assert_eq!(user_text(&msg), format!("{SUMMARY_HEADER}new summary"));
    Ok(())
}

#[tokio::test]
async fn marker_summary_used_as_carry_over_when_in_process_none() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists, and the in-process carry_over is None (restart). The
    // marker summary must be prepended to the summarizer input.
    let (_, store, marker_summary, _temp_dir) =
        seeded_marker_store("conv-1", "carried forward summary").await?;
    let bus = Bus::new();
    let model = stream_model(&["new summary"]);
    let compactor = make_compactor_with_store(model.clone(), bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let prompt = last_request_prompt(&model);
    assert!(
        prompt.contains(&marker_summary),
        "marker summary must be prepended as carry_over on restart, got: {prompt}"
    );
    Ok(())
}

#[tokio::test]
async fn marker_present_still_emits_full_lifecycle_events() -> Result<()> {
    // -- Setup & Fixtures
    // A marker exists that already absorbed all the evicted messages. Compaction
    // must still emit the full Started/SummaryChunk/Completed lifecycle.
    let (_, store, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model, bus, None, store);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let started = rx.recv().await.map_err(|_| "should receive Started")?;
    assert!(
        matches!(started, CompactionEvent::Started { .. }),
        "first event should be Started, got {started:?}"
    );
    let chunk = rx.recv().await.map_err(|_| "should receive SummaryChunk")?;
    assert!(
        matches!(chunk, CompactionEvent::SummaryChunk { .. }),
        "expected a SummaryChunk after Started, got {chunk:?}"
    );
    let completed = rx.recv().await.map_err(|_| "should receive Completed")?;
    match &completed {
        CompactionEvent::Completed { summary_body, .. } => {
            assert_eq!(
                summary_body, "fresh LLM summary",
                "Completed must carry the fresh LLM summary"
            );
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn marker_present_still_writes_a_new_marker() -> Result<()> {
    // -- Setup & Fixtures
    let (store, store_arc, _summary, _temp_dir) =
        seeded_marker_store("conv-1", "cached summary text").await?;
    let bus = Bus::new();
    let model = stream_model(&["fresh LLM summary"]);
    let compactor = make_compactor_with_store(model, bus, None, store_arc);

    // -- Exec
    let evicted = vec![
        Message::user("m1"),
        Message::user("m2"),
        Message::user("m3"),
    ];
    compactor
        .compact("conv-1", &evicted, None, "test_source")
        .await
        .map_err(|e| format!("compact should succeed: {e}"))?;

    // -- Check
    let (_, entries) = store.load("conv-1").await?.ok_or("should find session")?;
    let markers: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e, StoreEntry::Marker(_)))
        .collect();
    assert_eq!(
        markers.len(),
        2,
        "compact must append a new marker even when one already exists, found {}",
        markers.len()
    );
    Ok(())
}
