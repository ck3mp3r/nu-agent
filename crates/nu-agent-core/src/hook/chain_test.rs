use std::sync::{Arc, Mutex};

use crate::bus::{Bus, WarningEvent};
use crate::compaction::CompactionParams;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::compaction::compactor::NuCompactor;
use crate::hook::doom_loop::DoomLoopState;
use crate::hook::output_repetition::RepetitionState;
use crate::hook::permission_resolver::PolicyPermissionResolver;
use crate::session::{CachedMemory, FsSessionStore, SessionStore, StoreEntry};
use crate::types::Message;
use futures::StreamExt;
use rig::agent::ModelHandle;
use rig::streaming::StreamingPrompt;
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
        Bus::default(),
        None,
    )
    .with_store(store.clone());
    let memory = Arc::new(CachedMemory::new(store));
    let bus = Bus::default();
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
    let bus = Bus::default();
    let mut rx = bus.compaction().subscribe();
    let empty_marker = Some(crate::session::CompactionMarker::new(
        "".to_string(),
        chrono::Utc::now(),
    ));

    // -- Exec
    let action = patch_from_marker(&[], &empty_marker, &bus).await;

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
    let compactor = NuCompactor::new(ModelHandle::new(model.clone()), Bus::default(), None)
        .with_store(store_arc.clone());

    let memory = Arc::new(CachedMemory::new(store_arc));
    let bus = Bus::default();
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
    let compactor =
        NuCompactor::new(ModelHandle::new(model), Bus::default(), None).with_store(store.clone());
    let memory = Arc::new(CachedMemory::new(store));
    let bus = Bus::default();
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
        matches!(rx.try_recv(), Err(crate::bus::TryRecvError::Empty)),
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

// ---------------------------------------------------------------------------
// Output-repetition hook behavior: warn-only intra-stream, cross-turn ladder
// ---------------------------------------------------------------------------

/// Test harness running the real `HookChain` through real rig streaming
/// agents. Rig's stream ends after ONE model turn, so cross-turn scenarios
/// build a FRESH agent per `turn()` call while the `HookChain` state Arcs
/// are shared across them — exactly how the executor runs production turns.
/// The hook's `on_model_select` routes every request to the queued
/// `shared_model`, so the builder's own model is never consulted.
struct RepetitionTurnFixture {
    bus: Bus,
    shared_model: Arc<std::sync::Mutex<ModelHandle>>,
    hook_state: HookState<FsSessionStore>,
    _temp_dir: TempDir, // keeps the store backing the memory alive
}

impl RepetitionTurnFixture {
    /// Build the fixture: shared hook state over a tempdir-backed memory, a
    /// queued scripted model, and a real bus.
    fn new() -> Result<Self> {
        let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
        let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
        let memory = Arc::new(CachedMemory::new(store));
        Ok(Self {
            bus: Bus::default(),
            shared_model: Arc::new(std::sync::Mutex::new(ModelHandle::new(
                MockCompletionModel::default(),
            ))),
            hook_state: HookState {
                circuit_breaker: Arc::new(std::sync::Mutex::new(
                    crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
                )),
                doom_state: Arc::new(std::sync::Mutex::new(DoomLoopState::default())),
                output_repetition: Arc::new(std::sync::Mutex::new(RepetitionState::default())),
                repetition_guard: true,
                shared_model: Arc::new(std::sync::Mutex::new(ModelHandle::new(
                    MockCompletionModel::default(),
                ))),
                memory,
                conversation_id: "chain-test-conv".to_string(),
                compaction: CompactionConfig {
                    compactor: NuCompactor::new(
                        ModelHandle::new(MockCompletionModel::text("summary")),
                        Bus::default(),
                        None,
                    ),
                    params: CompactionParams::default(),
                    threshold_tokens: None,
                },
                last_total_tokens: Arc::new(std::sync::Mutex::new(None)),
            },
            _temp_dir: temp_dir,
        })
    }

    /// Queue a script of one or more streaming turns on the shared model
    /// handle. A tool-call turn needs TWO model calls in the same script:
    /// the tool call, then the post-tool-result final text.
    fn queue_turn(&self, turns: Vec<Vec<MockStreamEvent>>) {
        *self.shared_model.lock().expect("model mutex poisoned") =
            ModelHandle::new(MockCompletionModel::from_stream_turns(turns));
    }

    /// Run one turn: build a fresh agent carrying this fixture's hook chain
    /// (fresh chain per call, but state Arcs are shared), drive the rig
    /// stream to completion, and return the stream items. `with_tool` adds a
    /// static tool to the agent (for tool-call turn scripts).
    async fn turn(
        &self,
        with_tool: bool,
    ) -> core::result::Result<
        Vec<core::result::Result<rig::agent::MultiTurnStreamItem, rig::agent::StreamingError>>,
        Box<dyn std::error::Error>,
    > {
        let hook = HookChain::new(
            self.bus.clone(),
            PolicyPermissionResolver {
                permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
                session_grants: Arc::new(std::sync::Mutex::new(
                    crate::tools::authz::SessionGrantCache::default(),
                )),
                closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
                mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
            },
            Arc::new(crate::tools::closure::ClosureRegistry::default()),
            Arc::new(crate::tools::handler::McpToolRegistry::empty()),
            None,
            HookState {
                circuit_breaker: self.hook_state.circuit_breaker.clone(),
                doom_state: self.hook_state.doom_state.clone(),
                output_repetition: self.hook_state.output_repetition.clone(),
                repetition_guard: self.hook_state.repetition_guard,
                shared_model: self.shared_model.clone(),
                memory: self.hook_state.memory.clone(),
                conversation_id: self.hook_state.conversation_id.clone(),
                compaction: self.hook_state.compaction.clone(),
                last_total_tokens: self.hook_state.last_total_tokens.clone(),
            },
        );
        let builder = rig::agent::AgentBuilder::from_model_handle(ModelHandle::new(
            MockCompletionModel::default(),
        ))
        .add_hook(hook);
        let agent = if with_tool {
            builder.tool(rig::test_utils::MockAddTool).build()
        } else {
            builder.build()
        };
        let mut stream = agent.stream_prompt("run").max_turns(16).await;
        let mut items = Vec::new();
        while let Some(item) = stream.next().await {
            items.push(item);
        }
        Ok(items)
    }
}

/// Drain warning messages from an EXISTING subscriber (must be subscribed
/// before the events are published — broadcast sends are not buffered for
/// later subscribers).
fn warnings(rx: &mut crate::bus::WarningRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(WarningEvent::Message { message }) = rx.try_recv() {
        out.push(message);
    }
    out
}

/// A script of `count` identical text deltas followed by a final response.
fn identical_text_turn(count: usize) -> Vec<MockStreamEvent> {
    let mut turn = vec![MockStreamEvent::Text("I will do the thing. ".to_string()); count];
    turn.push(MockStreamEvent::final_response_with_default_usage());
    turn
}

/// Intra-stream detection stops the stream on the FIRST detection: a
/// repetitive stream is killed mid-turn with a PromptCancelled carrying the
/// `OUTPUT_REPETITION_STOP_PREFIX` reason, and NO hook-side warning is
/// published (the executor surfaces the stop reason).
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_on_first_detection() -> Result<()> {
    // -- Setup & Fixtures
    // 12 deltas x 21 bytes = 252 bytes: the growth gate opens at delta 5
    // (105 bytes) where the suffix check detects the 5-aligned 21-byte-period
    // repetition → `check_aggregated` returns Stop → the stream dies
    // mid-turn. No retry turn is queued (the executor's stop-to-steering
    // retry is exercised in executor tests, not here).
    let fx = RepetitionTurnFixture::new()?;
    fx.queue_turn(vec![identical_text_turn(12)]);
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec: drive the stream; the repetitive turn must die mid-stream.
    let items = fx.turn(false).await?;

    // -- Check: the stream errors with a PromptCancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error = error_stream.ok_or("the first intra-stream detection must stop the run")?;
    let rig::agent::StreamingError::Prompt(boxed) = error else {
        return Err("the mid-stream stop must surface as a Prompt streaming error".into());
    };
    let rig::completion::PromptError::PromptCancelled { reason, .. } = *boxed else {
        return Err("the mid-stream stop must surface PromptCancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}

/// Cross-turn ladder via `on_model_turn_finished`: turns 1-4 identical are
/// below threshold (each turn completes), the 5th is First →
/// `retry_with_feedback` (+ warning), 6th/7th Backoff (retry + warning), the
/// 8th Stop (the turn's stream errors with PromptCancelled) + no warning.
/// Rig's stream ends after one model turn, so each scripted turn is queued
/// and run separately through the SHARED hook state — exactly how the
/// executor runs production turns.
#[tokio::test]
async fn on_model_turn_finished_cross_turn_ladder() -> Result<()> {
    // -- Setup & Fixtures
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec & Check
    let mut total_warnings = 0usize;
    for turn in 1..=8usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        // Each `warnings()` drain is destructive; tally what this turn fired.
        let turn_warnings = warnings(&mut warning_rx);
        total_warnings += turn_warnings.len();
        match turn {
            1..=4 => {
                assert!(
                    items.iter().any(|item| matches!(
                        item,
                        Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_))
                    )),
                    "turn {turn} below threshold must complete, got {items:?}"
                );
                assert!(
                    !items.iter().any(|item| item.is_err()),
                    "turn {turn} below threshold must not error, got {items:?}"
                );
                assert!(
                    turn_warnings.is_empty(),
                    "turn {turn} below threshold must not warn, got {turn_warnings:?}"
                );
            }
            5..=7 => {
                // First + Backoff x2: the turn is retried (ModelTurnRetried
                // marker) and the retry consumes the queued next turn.
                assert!(
                    items.iter().any(|item| matches!(
                        item,
                        Ok(rig::agent::MultiTurnStreamItem::ModelTurnRetried { .. })
                    )),
                    "turn {turn} must be retried (ladder escalation), got {items:?}"
                );
                assert_eq!(
                    turn_warnings.len(),
                    1,
                    "turn {turn} must publish exactly one steering warning, got {turn_warnings:?}"
                );
            }
            8 => {
                // Stop: the stream ends with a PromptCancelled error and NO
                // hook-side warning (the executor surfaces the reason).
                assert!(
                    turn_warnings.is_empty(),
                    "turn 8 (Stop) must not publish a hook-side warning, got {turn_warnings:?}"
                );
                let mut stopped = None;
                for item in items {
                    if let Err(err) = item {
                        stopped = Some(err);
                        break;
                    }
                }
                let error_stream = stopped.ok_or("turn 8 must stop the run (stream error)")?;
                let rig::agent::StreamingError::Prompt(boxed) = error_stream else {
                    return Err("turn 8 stop must surface as a Prompt streaming error".into());
                };
                let rig::completion::PromptError::PromptCancelled { reason, .. } = *boxed else {
                    return Err("turn 8 stop must surface PromptCancelled".into());
                };
                assert!(
                    reason
                        .starts_with(crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX),
                    "turn 8 stop reason must carry the repetition stop prefix, got {reason}"
                );
            }
            _ => unreachable!(),
        }
    }
    // Total warnings across the ladder: First + 2x Backoff steering, none for Stop.
    assert_eq!(
        total_warnings, 3,
        "exactly First + 2x Backoff warnings must be published"
    );

    Ok(())
}

/// A tool-call turn resets the cross-turn counter: 4 identical text turns + a
/// tool-call turn + text turns again → the ladder never escalates beyond
/// First (the counter was reset mid-sequence).
#[tokio::test]
async fn on_model_turn_finished_tool_call_resets() -> Result<()> {
    // -- Setup & Fixtures
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec & Check: 4 identical text turns are below threshold.
    for turn in 1..=4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            items
                .iter()
                .any(|item| matches!(item, Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)))),
            "text turn {turn} below threshold must complete, got {items:?}"
        );
    }
    // A tool-call turn: content carries a tool call, resetting the counter.
    // The turn needs two model calls (tool call + post-result final), so the
    // agent is built WITH the mock add tool and the script carries both calls.
    fx.queue_turn(vec![
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::text("tool done"),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let items = fx.turn(true).await?;
    assert!(
        !items.iter().any(|item| item.is_err()),
        "tool-call turn must not error, got {items:?}"
    );
    // 4 more identical text turns after the reset must not trip.
    for turn in 1..=4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            items
                .iter()
                .any(|item| matches!(item, Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)))),
            "post-reset turn {turn} must complete (counter was reset), got {items:?}"
        );
    }
    // The ladder was never consumed (all turns continued).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "a tool-call turn resets the ladder: no detection should fire"
    );

    Ok(())
}

/// Non-repetitive turns: no warning, no retry, every turn completes.
#[tokio::test]
async fn on_model_turn_finished_non_repetitive_continues_without_warning() -> Result<()> {
    // -- Setup & Fixtures
    let texts = [
        "First response with some content.",
        "Second response, quite different.",
        "Third response, again different.",
        "Fourth response, still distinct.",
        "Fifth response, unique as well.",
        "Sixth response, yet another one.",
    ];
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec & Check
    for (i, text) in texts.iter().enumerate() {
        fx.queue_turn(vec![vec![
            MockStreamEvent::Text((*text).to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let items = fx.turn(false).await?;
        assert!(
            items.iter().all(|item| item.is_ok()),
            "distinct turn {i} must complete, got {items:?}"
        );
    }

    // -- Check
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "distinct turns must not warn"
    );

    Ok(())
}

/// Intra-stream stop with a PRE-ESCALATED ladder (count 4): the cross-turn
/// ladder is driven to Stop level (turns 1-8 like
/// `on_model_turn_finished_cross_turn_ladder`); the next repetitive stream is
/// still killed MID-TURN by `on_text_delta` returning stop with the
/// `OUTPUT_REPETITION_STOP_PREFIX` reason — the same PromptCancelled
/// mechanism as the Esc-Esc cancel path. No hook-side warning is sent (the
/// executor surfaces the stop reason).
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_with_pre_escalated_ladder() -> Result<()> {
    // -- Setup & Fixtures: drive the shared ladder to Stop level.
    let fx = RepetitionTurnFixture::new()?;
    for _ in 0..8usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let _ = fx.turn(false).await?;
    }

    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec: a fresh repetitive turn must die mid-stream, not complete.
    fx.queue_turn(vec![identical_text_turn(12)]);
    let items = fx.turn(false).await?;

    // -- Check: the stream stops with a PromptCancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error = error_stream.ok_or("turn at Stop level must stop the run (stream error)")?;
    let rig::agent::StreamingError::Prompt(boxed) = error else {
        return Err("the Stop-level stop must surface as a Prompt streaming error".into());
    };
    let rig::completion::PromptError::PromptCancelled { reason, .. } = *boxed else {
        return Err("the Stop-level stop must surface PromptCancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}

/// Intra-stream at ANY ladder level stops the stream mid-stream: even with
/// the cross-turn ladder escalated (count 1), detection returns Stop and the
/// stream dies mid-turn with the `OUTPUT_REPETITION_STOP_PREFIX` reason.
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_at_any_ladder_level() -> Result<()> {
    // -- Setup & Fixtures: drive the shared ladder to First (count 1).
    let fx = RepetitionTurnFixture::new()?;
    // Turns 1-4 are below the cross-turn threshold and complete.
    for _ in 0..4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            !items.iter().any(|item| item.is_err()),
            "fixture turns below the threshold must not error, got {items:?}"
        );
    }
    // Turn 5 is the cross-turn First escalation: the boundary retry consumes
    // a queued distinct turn and completes the run.
    fx.queue_turn(vec![
        identical_text_turn(1),
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let items = fx.turn(false).await?;
    assert!(
        items.iter().any(|item| matches!(
            item,
            Ok(rig::agent::MultiTurnStreamItem::ModelTurnRetried { .. })
        )),
        "fixture: the 5th turn must escalate to First, got {items:?}"
    );

    // Subscribe BEFORE driving the stream.
    let mut warning_rx = fx.bus.warning().subscribe();

    // -- Exec: the next repetitive turn must STILL stop mid-stream
    // (escalation_count 1 < 4 does not matter — every detection stops).
    fx.queue_turn(vec![identical_text_turn(12)]);
    let items = fx.turn(false).await?;

    // -- Check: the stream stops with a PromptCancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error =
        error_stream.ok_or("a repetitive turn must stop mid-stream regardless of ladder level")?;
    let rig::agent::StreamingError::Prompt(boxed) = error else {
        return Err("the mid-stream stop must surface as a Prompt streaming error".into());
    };
    let rig::completion::PromptError::PromptCancelled { reason, .. } = *boxed else {
        return Err("the mid-stream stop must surface PromptCancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}
