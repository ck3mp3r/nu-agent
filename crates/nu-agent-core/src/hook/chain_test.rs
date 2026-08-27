use std::sync::{Arc, Mutex};

use crate::bus::Bus;
use crate::compaction::CompactionParams;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::{NoopProgressUi, NuCompactor};
use crate::session::{CachedMemory, FsSessionStore, SessionStore, StoreEntry};
use crate::types::Message;
use rig::agent::ModelHandle;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use tempfile::TempDir;

use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A `SessionStore` whose `load` always fails, used to verify store errors are
/// surfaced (not swallowed) by `load_marker_context`.
#[derive(Clone)]
struct FailingStore;

#[derive(Debug)]
struct StoreError;

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store exploded")
    }
}

impl std::error::Error for StoreError {}

impl SessionStore for FailingStore {
    type Error = StoreError;

    async fn create(
        &self,
        _id: &str,
        _first_messages: &[Message],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn load(
        &self,
        _id: &str,
    ) -> core::result::Result<Option<(crate::session::SessionMetadata, Vec<StoreEntry>)>, Self::Error>
    {
        Err(StoreError)
    }

    async fn append(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn replace_entries(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn list(&self) -> core::result::Result<Vec<crate::session::SessionInfo>, Self::Error> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &str) -> core::result::Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn load_marker_context_surfaces_store_error_as_failed() -> Result<()> {
    // -- Setup & Fixtures
    let store = Arc::new(FailingStore);
    let compactor = NuCompactor::new(
        ModelHandle::new(rig::test_utils::MockCompletionModel::from_stream_turns([[
            rig::test_utils::MockStreamEvent::Text("summary".to_string()),
            rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
        ]])),
        NoopProgressUi,
        Bus::new(),
        None,
    )
    .with_store(store.clone());
    let memory = Arc::new(CachedMemory::new(store));
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();

    // -- Exec
    let (marker, messages) = load_marker_context(&compactor, memory.as_ref(), "conv-1", &bus).await;

    // -- Check
    assert!(
        marker.is_none(),
        "store error must fall back to no marker, got {marker:?}"
    );
    assert!(
        messages.is_empty(),
        "store error must fall back to no messages, got {messages:?}"
    );
    let failed = rx.recv().await.map_err(|_| "should receive Failed")?;
    assert!(
        matches!(
            &failed,
            CompactionEvent::Failed { message, .. }
                if message.contains("store exploded")
        ),
        "store error must be surfaced in a Failed event, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn patch_from_marker_with_empty_summary_emits_failed() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.compaction().subscribe();
    let empty_marker = Some(crate::session::CompactionMarker::new(
        "".to_string(),
        chrono::Utc::now(),
    ));

    // -- Exec
    let action = patch_from_marker(&[], &empty_marker, &bus);

    // -- Check
    assert!(
        action.is_none(),
        "an empty-summary marker must not produce a patch"
    );
    let failed = rx.recv().await.map_err(|_| "should receive Failed")?;
    assert!(
        matches!(
            &failed,
            CompactionEvent::Failed { message, .. }
                if message.contains("empty summary")
        ),
        "empty-summary marker must surface a Failed event, got {failed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn over_threshold_fires_requested_and_does_not_compact_synchronously() -> Result<()> {
    // -- Setup & Fixtures
    // A shared store holds the messages. The history is sized so the full
    // history is far above the token threshold. `decide_compaction` must fire a
    // `CompactionEvent::Requested { source: "auto" }` on the bus and NOT invoke
    // the summarizer LLM synchronously (the orchestrator runs compaction).
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = FsSessionStore::new(temp_dir.path().to_path_buf());
    let store_arc = Arc::new(store.clone());

    let history: Vec<Message> = (0..5)
        .map(|i| Message::user(format!("user message {i} ").repeat(400)))
        .collect();
    let entries: Vec<StoreEntry> = history.iter().cloned().map(StoreEntry::Message).collect();
    store
        .append("conv-1", &entries)
        .await
        .map_err(|_| "should append messages")?;

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("rolled-up summary".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(
        ModelHandle::new(model.clone()),
        NoopProgressUi,
        Bus::new(),
        None,
    )
    .with_store(store_arc.clone());

    let memory = Arc::new(CachedMemory::new(store_arc));
    let bus = Bus::new();
    let mut compaction_rx = bus.compaction().subscribe();
    let compaction = CompactionConfig {
        compactor,
        params: CompactionParams::default(),
        threshold_tokens: Some(100),
    };
    let last_total_tokens = Arc::new(Mutex::new(None));
    let prompt = Message::user("current user prompt");

    // -- Exec: first turn — no marker yet, full history over threshold.
    let action = decide_compaction(
        &history,
        &prompt,
        "conv-1",
        memory.as_ref(),
        &compaction,
        &last_total_tokens,
        &bus,
    )
    .await;

    // -- Check: a Requested event was fired and the LLM was NOT called.
    let requested = compaction_rx
        .recv()
        .await
        .map_err(|_| "should receive a Requested event")?;
    assert!(
        matches!(
            &requested,
            crate::bus::CompactionEvent::Requested { source } if source == "auto"
        ),
        "over-threshold turn must fire Requested {{ source: \"auto\" }}, got {requested:?}"
    );
    assert_eq!(
        model.requests().len(),
        0,
        "decide_compaction must NOT invoke the summarizer LLM synchronously"
    );
    assert!(
        action.is_none(),
        "with no marker present the hook must return None (continue_run)"
    );
    Ok(())
}

#[test]
fn nu_nonzero_exit_code_is_failure() {
    let result = resolve_success(
        "nu",
        true,
        r#"{"stdout":"","stderr":"error","exit_code":1}"#,
    );
    assert!(!result);
}

#[test]
fn nu_zero_exit_code_is_success() {
    let result = resolve_success("nu", true, r#"{"stdout":"ok","stderr":"","exit_code":0}"#);
    assert!(result);
}

#[test]
fn nu_parse_failure_falls_back_to_success() {
    let result = resolve_success("nu", true, "not json");
    assert!(result);
}

#[test]
fn other_tools_unaffected() {
    let result = resolve_success("read_file", true, r#"{"exit_code":1}"#);
    assert!(result);
}

#[test]
fn nu_base_failure_stays_failure() {
    let result = resolve_success("nu", false, r#"{"stdout":"","stderr":"","exit_code":0}"#);
    assert!(!result);
}

// ---------------------------------------------------------------------------
// Compaction threshold with real token count
// ---------------------------------------------------------------------------

/// Shared fixtures for the `decide_compaction` threshold tests: an in-memory
/// store (no markers), a compactor backed by a deterministic mock model, an
/// empty `last_total_tokens` slot, and the compaction config + bus.
struct DecideFixture {
    memory: Arc<CachedMemory<FsSessionStore>>,
    compaction: CompactionConfig<FsSessionStore>,
    bus: Bus,
    last_total_tokens: Arc<Mutex<Option<u64>>>,
}

/// Build the fixtures shared by the `decide_compaction` threshold tests.
fn decide_fixture() -> Result<DecideFixture> {
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("summary".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let compactor = NuCompactor::new(ModelHandle::new(model), NoopProgressUi, Bus::new(), None)
        .with_store(store.clone());
    let memory = Arc::new(CachedMemory::new(store));
    let bus = Bus::new();
    let compaction = CompactionConfig {
        compactor,
        params: CompactionParams::default(),
        threshold_tokens: Some(80_000),
    };
    let last_total_tokens = Arc::new(Mutex::new(None));
    Ok(DecideFixture {
        memory,
        compaction,
        bus,
        last_total_tokens,
    })
}

/// A short single-message history (so the per-turn context is not empty) plus a
/// short prompt. The context chars/4 estimate is far below the threshold; the
/// decision must be driven by `last_total_tokens` when it is `Some`.
fn small_history_and_prompt() -> (Vec<Message>, Message) {
    (
        vec![Message::user("short context message")],
        Message::user("hi"),
    )
}

#[tokio::test]
async fn decide_compaction_real_tokens_over_threshold_fires() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    let (history, prompt) = small_history_and_prompt();
    // Real count (90000) plus a small prompt estimate (> 80_000) → fires.
    *fx.last_total_tokens.lock().unwrap() = Some(90_000);
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    let action = decide_compaction(
        &history,
        &prompt,
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "real total_tokens over threshold must fire Requested, got {requested:?}"
    );
    assert!(
        action.is_none(),
        "no marker present so the hook returns None (continue_run)"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_real_tokens_under_threshold_does_not_fire() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    let (history, prompt) = small_history_and_prompt();
    // Real count (70000) plus a small prompt estimate (< 80_000) → does not fire.
    *fx.last_total_tokens.lock().unwrap() = Some(70_000);
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    let action = decide_compaction(
        &history,
        &prompt,
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check: no compaction requested, no marker → None.
    assert!(
        action.is_none(),
        "under-threshold turn with no marker must return None"
    );
    assert!(
        matches!(
            rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ),
        "under-threshold turn must not publish any compaction event"
    );
    Ok(())
}

#[tokio::test]
async fn run_compaction_empty_history_emits_completed_with_empty_summary() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    let result = run_compaction(
        &[],
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        "slash",
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    assert!(
        result.is_none(),
        "empty history must return None (nothing to compact)"
    );
    let completed = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .map_err(|_| "should receive Completed within timeout")?
        .map_err(|_| "should receive Completed")?;
    assert!(
        matches!(
            &completed,
            CompactionEvent::Completed {
                source,
                summary_preview,
                summary_body,
            } if source == "slash"
                && summary_preview.is_empty()
                && summary_body.is_empty()
        ),
        "empty history must emit Completed with empty summary, got {completed:?}"
    );
    Ok(())
}

#[tokio::test]
async fn decide_compaction_no_real_tokens_falls_back_to_context_estimate() -> Result<()> {
    // -- Setup & Fixtures
    let fx = decide_fixture()?;
    // `last_total_tokens` stays `None` (first turn). The context is sized so its
    // chars/4 estimate alone is over the threshold → fires via the fallback path.
    let history = vec![Message::user("x".repeat(400_000))];
    let prompt = Message::user("hi");
    let mut rx = fx.bus.compaction().subscribe();

    // -- Exec
    let action = decide_compaction(
        &history,
        &prompt,
        "conv-1",
        fx.memory.as_ref(),
        &fx.compaction,
        &fx.last_total_tokens,
        &fx.bus,
    )
    .await;

    // -- Check
    let requested = rx.recv().await.map_err(|_| "should receive Requested")?;
    assert!(
        matches!(&requested, CompactionEvent::Requested { source } if source == "auto"),
        "fallback estimate over threshold must fire Requested, got {requested:?}"
    );
    assert!(
        action.is_none(),
        "no marker present so the hook returns None (continue_run)"
    );
    Ok(())
}
