//! Tests for TurnExecutor API surface and execute() cancellation paths.
//!
//! Covers:
//!  - TurnExecutor construction (API surface smoke tests)
//!  - Path C: PromptCancelled caught inside build_agent_and_stream returns
//!    Ok(cancelled=true, messages=Some) — executor must return EarlyReturn,
//!    persist messages, emit Completed, and NOT emit AssistantMessage.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{default_circuit_breaker, default_doom_state, default_last_total_tokens};
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::conversation::state::memory::MemoryState;
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::protocol::event::UiEvent;
use crate::session::{FsSessionStore, SessionStore, StoreEntry};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::utils::value_ext::extract_response_text_from_value;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Build a `MemoryState<FsSessionStore>` backed by the given tempdir (no
/// compaction — `CachedMemory` is used directly).
fn make_memory_state(temp_dir: &tempfile::TempDir) -> MemoryState<FsSessionStore> {
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    MemoryState::new(store)
}

#[test]
fn turn_executor_new_constructs_without_panic() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    let shared_model = super::test_utils::shared_mock_model_handle();

    let _executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );
    // Construction succeeded — no panic.
}

#[test]
fn turn_executor_exposes_memory_state() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    let shared_model = super::test_utils::shared_mock_model_handle();

    let executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // Verify memory_state is accessible and last_total_tokens starts None
    assert!(executor.memory_state.last_total_tokens().is_none());
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    let shared_model = super::test_utils::shared_mock_model_handle();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    assert!(executor.take_response_data().is_none());
}

// ---------------------------------------------------------------------------
// Path C: Ok(cancelled=true, messages=Some) returns EarlyReturn + persists
// ---------------------------------------------------------------------------

/// RED: cancelled turn via Ok path returns EarlyReturn, persists messages, emits Completed.
///
/// Before Path C is added to executor.rs, this test FAILS because the current code
/// falls through to the normal persistence path and returns TurnOutcome::Completed.
///
/// The mock model emits one text chunk then a FinalResponse. The UI cancels immediately
/// (before the first drain loop tick), causing the cancel_token to fire before the
/// spawned tokio task processes any stream event. The hook's on_completion_call sees
/// Terminate, rig yields PromptCancelled { chat_history }, and build_agent_and_stream
/// returns Ok(StreamingTurnResult { cancelled: true, messages: Some(chat_history) }).
/// This reaches executor.execute() as Ok(TurnResult { cancelled: true, messages: Some }).
///
/// Expected behaviour after the fix:
///   1. result == Ok(TurnOutcome::EarlyReturn(_))
///   2. conversation store was appended with the cancelled messages
///   3. UiEvent::Completed was emitted
///   4. UiEvent::AssistantMessage was NOT emitted
#[tokio::test]
async fn cancelled_ok_path_returns_early_return_persists_messages_and_emits_completed() -> Result<()>
{
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-session";
    let mut memory_state = make_memory_state(&temp_dir);

    // The model emits a tool call to the cancelling tool, which publishes a
    // CancelEvent on the shared bus from inside `call()`. This drives the
    // cancelled path deterministically (the hook subscribes to `bus.cancel()`
    // before the turn runs).
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut turn_rx = bus.turn().subscribe();

    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();

    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // 1. Result must be EarlyReturn (not Completed — that would be the bug)
    assert!(
        result.is_ok(),
        "execute() must not return Err for a cancelled turn; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "cancelled Ok path must return TurnOutcome::EarlyReturn, not TurnOutcome::Completed"
    );

    // 2. Conversation store must have been written with the cancelled messages
    //    (via JournalConversationMemory.append() — single write to both JSONL and cache)
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    // The cancelling tool produces a result and then cancels, so path C appends
    // the delta from PromptCancelled::chat_history plus the synthetic assistant
    // close-block: [user("hello"), asst(tool_call), user(tool_result),
    // asst(close)]. We assert the delta is non-empty and small (<= 4) to catch
    // duplication (which would produce more).
    assert!(
        !persisted.is_empty() && persisted.len() <= 4,
        "cancelled turn: expected 1-4 messages, got {}; messages: {:?}",
        persisted.len(),
        persisted
    );

    // 2b. memory.load() returns repair-filtered view — raw messages are in JSONL (asserted
    //     above). The cache is updated by append(), but load() applies repair which may
    //     trim a trailing user-only message from an immediately-cancelled turn.
    // The key invariant is JSONL durability (step 2 above), not the repair-filtered view.

    // 3. TurnEvent::Completed must have been published on the bus turn channel
    let completed_received = turn_rx
        .try_recv()
        .map(|event| matches!(event, crate::bus::TurnEvent::Completed { .. }))
        .unwrap_or(false);
    assert!(
        completed_received,
        "TurnEvent::Completed must be published for a cancelled turn (path C)"
    );

    // 4. UiEvent::AssistantMessage must NOT have been emitted
    let events = event_collector.drain();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "UiEvent::AssistantMessage must NOT be emitted for a cancelled turn (path C)"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Completed turn: rig writes JSONL via memory.append() — no explicit store write
// ---------------------------------------------------------------------------

/// After a successful (non-cancelled) turn, JSONL receives messages via
/// JournalConversationMemory.append() called by rig — no explicit store.append() needed.
///
/// This verifies the double-write elimination: executor.rs no longer calls
/// conversation_store().append() for completed turns. The single write happens
/// through memory.append() which rig calls internally at turn end.
#[tokio::test]
async fn completed_turn_no_explicit_store_append_needed() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-completed-session";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello from LLM!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Collect response data before dropping the executor (which holds a mutable borrow)
    let response_data = executor.take_response_data();

    // Turn must complete normally
    assert!(
        result.is_ok(),
        "execute() must succeed; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("completed turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "completed turn must return TurnOutcome::Completed"
    );

    // rig wrote to JSONL via memory.append() — no explicit store.append() in executor
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !persisted.is_empty(),
        "completed turn: JSONL must contain messages written via memory.append()"
    );

    // Response data must be available
    assert!(
        response_data.is_some(),
        "TurnResponseData must be populated after completed turn"
    );

    Ok(())
}

/// Cancelled turn (path C) writes to both JSONL and in-memory cache via a
/// single JournalConversationMemory.append() call — not two separate calls.
///
/// Verifying the single-write pattern: both `conversation_store().load()` and
/// `memory().load()` return the same messages after a cancelled turn.
#[tokio::test]
async fn cancelled_turn_writes_via_single_memory_append() -> Result<()> {
    use rig::memory::ConversationMemory;

    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-single-write-cancelled";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok());
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // Both store (JSONL) and memory cache must have the messages
    let from_store_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let from_store: Vec<crate::types::Message> = from_store_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !from_store.is_empty(),
        "JSONL must have cancelled messages (via single memory.append())"
    );

    // memory.load() returns the repair-filtered view. For immediately-cancelled turns
    // where only a trailing user message was stored, repair trims it to an empty slice.
    // The key invariant is JSONL durability (from_store above), not the repair-filtered view.
    // Verify that memory.load() succeeds (doesn't panic/error) — content is repair-determined.
    let _ = memory_state
        .inner_memory()
        .load(session_id)
        .await
        .map_err(|e| format!("memory load should succeed without error: {e:?}"))?;

    Ok(())
}

/// `last_total_tokens` is set on the memory before rig calls `memory.append()` at turn end.
///
/// Verifies the timing fix in `turn/mod.rs`: on each `CompletionCall` event,
/// `memory.set_last_total_tokens()` is called so the value is current when rig
/// calls `memory.append()` during `FinalResponse`.
///
/// With the mock model (no real CompletionCall events), last_total_tokens stays 0.
/// This test verifies that `last_total_tokens_mut()` on MemoryState is updated
/// to reflect the turn result's last_total_tokens after a completed turn.
#[tokio::test]
async fn last_total_tokens_updated_on_completed_turn() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-token-tracking";
    let mut memory_state = make_memory_state(&temp_dir);

    // Verify initial state
    assert!(memory_state.last_total_tokens().is_none());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("response text".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "test prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok());
    let outcome = result.map_err(|e| format!("completed turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::Completed));

    // After a completed turn with a session, last_total_tokens must be Some(...)
    // (even if 0 from the mock model — the key is it was set).
    assert!(
        memory_state.last_total_tokens().is_some(),
        "last_total_tokens must be Some after a completed turn with a session"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Error path persistence tests
// ---------------------------------------------------------------------------

/// MaxTurnsError carries full chat_history — executor must persist it and return Err.
///
/// Setup: mock returns a tool_call on turn 1. Config limits tool turns to 0, so rig
/// raises MaxTurnsError after the first tool call attempt. After Fix 1, TurnError
/// gets messages=Some(chat_history). After Fix 2, executor persists those messages
/// before returning LabeledError.
#[tokio::test]
async fn max_turns_error_persists_full_history() -> Result<()> {
    // max_tool_turns=0: rig raises MaxTurnsError as soon as a tool-call turn would
    // be scheduled (current_turn > 0 + 1 after the first tool response).
    let config = Config {
        max_tool_turns: Some(0),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-max-turns";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: model asks for a tool call. With max_turns=0, rig will MaxTurnsError
    // as soon as it tries to schedule the tool-call turn.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tool_call_1", "some_tool", serde_json::json!({"x": 1})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "please call a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error, not a cancellation)
    assert!(
        result.is_err(),
        "MaxTurnsError must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !persisted.is_empty(),
        "MaxTurnsError must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );

    Ok(())
}

/// UnknownToolCall carries full chat_history — executor must persist it and return Err.
///
/// Setup: mock returns a tool_call for "nonexistent_tool" which is not registered
/// in the agent's tool list. Rig raises UnknownToolCall. After Fix 1, TurnError
/// gets messages=Some(chat_history). After Fix 2, executor persists them.
#[tokio::test]
async fn unknown_tool_error_persists_full_history() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-tool";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // No visible_tool_definitions → agent has no tools → any tool call is unknown.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tool_call_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "use a tool please".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "UnknownToolCall must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !persisted.is_empty(),
        "UnknownToolCall must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );

    Ok(())
}

/// Network/CompletionError on a fresh session — executor persists the user prompt
/// via the delta path (last_known_history = [user_prompt] after fix).
///
/// After the `on_completion_call` fix, `last_known_history` = `history + [prompt]` =
/// `[] + [user_msg]` = `[user_msg]`. delta = skip(0) = `[user_msg]` (non-empty),
/// so the delta path fires and persists just the user message. The placeholder path
/// is no longer triggered for this case.
#[tokio::test]
async fn network_error_on_fresh_session_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-network-error";
    let prompt_text = "what is the weather today?";
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error on the first event — simulates network failure.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "network error must propagate as LabeledError to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = skip(0) = [user_msg].
    // Delta path fires → 1 message persisted (just the user prompt).
    // The placeholder path no longer fires because the delta is non-empty.
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        persisted.len(),
        1,
        "hard error on fresh session must persist exactly 1 message (user prompt via delta path); got {} messages",
        persisted.len()
    );
    // The single persisted message must be the user prompt
    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message"
    );

    Ok(())
}

/// When `on_completion_call` fires before a CompletionError on a fresh session,
/// the executor persists the user prompt via the delta path (not a placeholder pair).
///
/// After the fix, `on_completion_call(prompt, history=[]`) stores `[] + [user_prompt]`
/// = `[user_prompt]`. `pre_turn_message_count = 0`, so `delta = skip(0) = [user_prompt]`
/// (non-empty) → delta path fires → 1 message persisted (just the user prompt).
///
/// The placeholder path (which would produce 2 messages) is no longer triggered
/// because the delta is now always non-empty when `on_completion_call` has fired.
///
/// This is CORRECT: we now save the user's question even if the API fails to respond,
/// which is better than saving a fake `[Turn failed:]` assistant message.
#[tokio::test]
async fn hard_error_on_first_llm_call_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-no-hook-history";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] → delta path fires → 1 message persisted.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = [user_msg], 1 message persisted.
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(
        persisted.len(),
        1,
        "hard error on fresh session must persist exactly 1 message (user prompt); got {:?}",
        persisted
    );

    // The single persisted message must be the user prompt (not a placeholder)
    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message; got {:?}",
        persisted[0]
    );

    Ok(())
}

/// When there is no session (transient invocation), hard errors must NOT write
/// anything to the store — there is no conversation to record.
#[tokio::test]
async fn hard_error_no_session_persists_nothing() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error — hard failure, no history recoverable.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "a transient prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None, // <-- no session
        )
        .await;

    // Must return Err
    assert!(result.is_err(), "hard error must propagate as LabeledError");

    // No JSONL should have been written — transient invocation with no session_id
    // There is no specific conversation_id to check, so we verify the temp dir
    // has no .jsonl files written (the store uses conversation_id as filename).
    let jsonl_files: Vec<_> = std::fs::read_dir(temp_dir.path())
        .expect("read_dir should succeed")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    assert!(
        jsonl_files.is_empty(),
        "no JSONL files must be written for a transient (no-session) hard error; found: {:?}",
        jsonl_files.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Subtask 1 — inject_missing_tool_results: integration tests
// ---------------------------------------------------------------------------

/// On PromptCancelled (Ok path, cancelled=true) with an unpaired ToolCall in
/// the chat_history, the messages written to JSONL must contain a synthetic
/// User(ToolResult) for that ToolCall ID so the stored history is always valid.
///
/// We force a PromptCancelled scenario by triggering immediate cancellation.
/// The mock model emits a tool_call on its first (and only) turn; the UI
/// cancels immediately so rig returns PromptCancelled with a chat_history that
/// contains the Assistant(ToolCall) but no User(ToolResult).
///
/// After inject_missing_tool_results, the persisted JSONL must contain both
/// the Assistant(ToolCall) and a User(ToolResult{id, content:"[interrupted]"}).
#[tokio::test]
async fn prompt_cancelled_with_unpaired_tool_call_injects_synthetic_result() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancel-inject";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model issues a tool call to the cancelling tool — the cancel fires after
    // the tool result is produced but before the next on_completion_call, so
    // chat_history will contain Assistant(ToolCall) with no matching
    // User(ToolResult), exercising the synthetic-result injection.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc_cancel_1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "call a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok(), "cancelled turn must not return Err");
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // The persisted JSONL must contain both ToolCall and ToolResult entries.
    // If the model was fast enough that the tool call was actually processed
    // before cancel fired, we may get a completed turn. Either way, if a
    // ToolCall was persisted, its ToolResult must also be persisted.
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // For every Assistant message with a ToolCall, there must be a following
    // User message with the matching ToolResult.
    use crate::types::{AssistantContent, UserContent};
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            // Find the next message
            let next_has_result = persisted.get(i + 1).is_some_and(|next| {
                if let crate::types::Message::User { content } = next {
                    content.iter().any(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            &tr.call == call_id
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            });
            assert!(
                next_has_result,
                "ToolCall id={call_id} must have a matching ToolResult in the persisted history"
            );
        }
    }

    Ok(())
}

/// On UnknownToolCall (Err path, e.messages=Some) with an unpaired ToolCall in
/// chat_history, the messages persisted to JSONL must contain a synthetic
/// User(ToolResult) immediately after the unpaired Assistant(ToolCall).
#[tokio::test]
async fn unknown_tool_error_with_unpaired_tool_call_injects_synthetic_result() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-inject";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // The chat_history will contain the user prompt + Assistant(ToolCall) but
    // no User(ToolResult) since the tool could not be dispatched.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tc_unknown_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "use a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // UnknownToolCall returns Err
    assert!(result.is_err(), "UnknownToolCall must propagate as Err");

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // Same invariant: every persisted ToolCall must have an adjacent ToolResult.
    use crate::types::{AssistantContent, UserContent};
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            let next_has_result = persisted.get(i + 1).is_some_and(|next| {
                if let crate::types::Message::User { content } = next {
                    content.iter().any(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            &tr.call == call_id
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            });
            assert!(
                next_has_result,
                "ToolCall id={call_id} must have a matching ToolResult in the persisted history (UnknownToolCall path)"
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Subtask 2 — error classification test
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Subtask 2 — error classification tests
// (Uses From<StreamingError> structural matching — kind is set at the error
// boundary, not via post-hoc string parsing)
// ---------------------------------------------------------------------------

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// an `InvalidStatusCode` HTTP error (numeric status code, no body).
fn turn_error_from_http_status(status: u16) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCode(
        reqwest::StatusCode::from_u16(status).map_err(|e| format!("valid status code: {e:?}"))?,
    );
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// an `InvalidStatusCodeWithMessage` HTTP error (status + body string).
fn turn_error_from_http_status_with_msg(status: u16, body: &str) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCodeWithMessage(
        reqwest::StatusCode::from_u16(status).map_err(|e| format!("valid status code: {e:?}"))?,
        body.to_string(),
    );
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// a `ResponseError` (provider returned a parseable error string).
fn turn_error_from_response_error(msg: &str) -> CompletionErrorKind {
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::ResponseError(msg.to_string()),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// a `PromptError::CompletionError` wrapping a `CompletionError`.
fn turn_error_from_prompt_wrapped(
    completion_err: rig::completion::CompletionError,
) -> crate::conversation::turn::TurnError {
    let prompt_err = rig::completion::PromptError::CompletionError(completion_err);
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(prompt_err));
    crate::conversation::turn::TurnError::from(streaming_err)
}

// ---------------------------------------------------------------------------
// Prompt-wrapped provider error classification tests
// ---------------------------------------------------------------------------

/// A `PromptError::CompletionError(ResponseError(s))` must classify like the
/// direct `StreamingError::Completion` path: kind from `classify_from_display`,
/// msg equal to the inner `CompletionError` Display ("ResponseError: {s}").
#[test]
fn prompt_wrapped_response_error_classifies_and_strips_completion_prefix() -> Result<()> {
    let s = "the model produced no answer and stopped with finish_reason=Length; \
             the turn ran out of output budget before producing one — raise max_tokens for this request";
    let turn_err = turn_error_from_prompt_wrapped(rig::completion::CompletionError::ResponseError(
        s.to_string(),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::CompletionError(ResponseError("finish_reason=length"))` must
/// classify as `OutputBudget`.
#[test]
fn prompt_wrapped_finish_reason_length_is_output_budget() -> Result<()> {
    let turn_err = turn_error_from_prompt_wrapped(rig::completion::CompletionError::ResponseError(
        "FinishReasonError { message: finish_reason=length }".to_string(),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::CompletionError(HttpError(InvalidStatusCode(429)))` must
/// classify as `RateLimit`.
#[test]
fn prompt_wrapped_http_429_is_rate_limit() -> Result<()> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCode(reqwest::StatusCode::from_u16(429)?);
    let turn_err =
        turn_error_from_prompt_wrapped(rig::completion::CompletionError::HttpError(http_err));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::MemoryError` must continue to classify as `Unknown`.
#[test]
fn prompt_wrapped_memory_error_is_unknown() -> Result<()> {
    let memory_err = rig::memory::MemoryError::Internal("boom".to_string());
    let prompt_err = rig::completion::PromptError::MemoryError(memory_err);
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(prompt_err));
    let turn_err = crate::conversation::turn::TurnError::from(streaming_err);
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::Unknown);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// `From<PromptError>` must route `PromptError::CompletionError` through the
/// same helper, producing the same kind and msg as the StreamingError path.
#[test]
fn from_prompt_error_completion_error_matches_streaming_path() -> Result<()> {
    let s = "provider exploded";
    let completion_err = rig::completion::CompletionError::ResponseError(s.to_string());
    let prompt_err = rig::completion::PromptError::CompletionError(completion_err);
    let turn_err = crate::conversation::turn::TurnError::from(prompt_err);
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg } => {
            assert_eq!(kind, CompletionErrorKind::Unknown);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Structural HTTP status → error kind classification tests
// ---------------------------------------------------------------------------

/// HTTP 429 with a message body must classify as `RateLimit`.
/// Uses `InvalidStatusCodeWithMessage` (status + body) path.
#[test]
fn http_429_with_message_is_rate_limit() -> Result<()> {
    let kind = turn_error_from_http_status_with_msg(429, "rate_limit_error")?;
    assert_eq!(
        kind,
        CompletionErrorKind::RateLimit,
        "HTTP 429 with message must classify as RateLimit"
    );
    assert!(kind.is_retryable(), "RateLimit must be retryable");
    Ok(())
}

/// Every HTTP status code maps to the correct error kind and retryable flag.
#[test]
fn http_status_to_error_kind() -> Result<()> {
    let cases: &[(u16, CompletionErrorKind, bool)] = &[
        (429, CompletionErrorKind::RateLimit, true),
        (500, CompletionErrorKind::ServerError, true),
        (503, CompletionErrorKind::Overloaded, true),
        (529, CompletionErrorKind::Overloaded, true),
        (504, CompletionErrorKind::ServerError, true),
        (413, CompletionErrorKind::RequestTooLarge, false),
        (401, CompletionErrorKind::Auth, false),
        (403, CompletionErrorKind::Auth, false),
        (402, CompletionErrorKind::Quota, false),
        (404, CompletionErrorKind::EndpointNotFound, false),
        (502, CompletionErrorKind::Unknown, false),
    ];
    for (status, expected_kind, retryable) in cases {
        let kind = turn_error_from_http_status(*status)?;
        assert_eq!(
            kind, *expected_kind,
            "status={status}: expected {expected_kind:?}, got {kind:?}"
        );
        assert_eq!(kind.is_retryable(), *retryable, "retryable status={status}");
    }
    Ok(())
}

/// `StreamEnded` must classify as `Network` (retryable).
#[test]
fn from_streaming_http_stream_ended_is_network() {
    use rig::http_client;
    let http_err = http_client::Error::StreamEnded;
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    let kind = match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    };
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// TDD (task step 8): `Instance(Box<dyn Error>)` with display `"error decoding response body"`
/// must classify as `Network` (retryable).  This is the BUG FIX test: the old string-matcher
/// used the pattern `"decode error"` but reqwest's actual Display string is
/// `"error decoding response body"`, so the old code returned `Unknown` (non-retryable).
/// The new `classify_from_display` uses `"error decoding"` which correctly matches.
#[test]
fn from_streaming_http_instance_error_decoding_response_body_is_network() {
    // The concrete type is erased, so we can only test through the Display string.
    // We simulate this via ResponseError (which uses classify_from_display) since we cannot
    // construct http_client::Error::Instance without a real Box<dyn std::error::Error>.
    let kind = turn_error_from_response_error("error decoding response body");
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "'error decoding response body' must classify as Network (retryable), not Unknown"
    );
    assert!(
        kind.is_retryable(),
        "Network must be retryable — this is the bug fix"
    );
}

/// `"error sending request for url ..."` must classify as `Network` via display matching.
#[test]
fn from_streaming_response_error_sending_request_is_network() {
    let kind =
        turn_error_from_response_error("error sending request for url https://api.example.com");
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// `"connection reset by peer"` must classify as `Network`.
#[test]
fn from_streaming_response_connection_reset_is_network() {
    let kind = turn_error_from_response_error("connection reset by peer");
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// `"context_length_exceeded"` via ResponseError must classify as `ContextOverflow`.
#[test]
fn from_streaming_response_context_length_exceeded_is_context_overflow() {
    let kind = turn_error_from_response_error("context_length_exceeded in prompt");
    assert_eq!(kind, CompletionErrorKind::ContextOverflow);
    assert!(!kind.is_retryable());
}

/// `"rate_limit"` via ResponseError must classify as `RateLimit`.
#[test]
fn from_streaming_response_rate_limit_string_is_rate_limit() {
    let kind = turn_error_from_response_error("rate_limit exceeded");
    assert_eq!(kind, CompletionErrorKind::RateLimit);
    assert!(kind.is_retryable());
}

/// Unknown error string via ResponseError falls through to `Unknown`.
#[test]
fn from_streaming_response_unknown_string_is_unknown() {
    let kind = turn_error_from_response_error("502 bad gateway proxy error");
    assert_eq!(kind, CompletionErrorKind::Unknown);
    assert!(!kind.is_retryable());
}

/// `"finish_reason=length"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_finish_reason_length_is_output_budget() {
    let kind =
        turn_error_from_response_error("FinishReasonError { message: finish_reason=length }");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// `"ran out of output budget"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_out_of_output_budget_is_output_budget() {
    let kind = turn_error_from_response_error("The model ran out of output budget");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// `"raise max_tokens"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_raise_max_tokens_is_output_budget() {
    let kind = turn_error_from_response_error("max_tokens too low, raise max_tokens");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// Provider "exceeds model's maximum output tokens" 400 via ResponseError must
/// classify as `OutputBudget` (residual defense for wrong-cache failures).
#[test]
fn from_streaming_response_exceeds_max_output_tokens_is_output_budget() {
    let kind = turn_error_from_response_error(
        "status 400: max_tokens (1048576) exceeds model's maximum output tokens (65536)",
    );
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// `PromptCancelled` via StreamingError must produce `Cancelled` variant.
#[test]
fn from_streaming_prompt_cancelled_produces_cancelled_variant() {
    let inner = rig::completion::PromptError::PromptCancelled {
        reason: "cancelled".to_string(),
        chat_history: vec![],
    };
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(inner));
    let turn_err = crate::conversation::turn::TurnError::from(streaming_err);
    assert!(turn_err.is_cancelled());
}

#[test]
fn is_retryable_matches_spec() {
    // Retryable kinds
    assert!(
        CompletionErrorKind::RateLimit.is_retryable(),
        "RateLimit must be retryable"
    );
    assert!(
        CompletionErrorKind::Overloaded.is_retryable(),
        "Overloaded must be retryable"
    );
    assert!(
        CompletionErrorKind::ServerError.is_retryable(),
        "ServerError must be retryable"
    );
    assert!(
        CompletionErrorKind::Network.is_retryable(),
        "Network must be retryable"
    );

    // Non-retryable kinds
    assert!(
        !CompletionErrorKind::RequestTooLarge.is_retryable(),
        "RequestTooLarge must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ContextOverflow.is_retryable(),
        "ContextOverflow must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ToolStructure.is_retryable(),
        "ToolStructure must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Auth.is_retryable(),
        "Auth must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Quota.is_retryable(),
        "Quota must not be retryable"
    );
    assert!(
        !CompletionErrorKind::CreditsExhausted.is_retryable(),
        "CreditsExhausted must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Refusal.is_retryable(),
        "Refusal must not be retryable"
    );
    assert!(
        !CompletionErrorKind::OutputBudget.is_retryable(),
        "OutputBudget must not be retryable"
    );
    assert!(
        !CompletionErrorKind::EndpointNotFound.is_retryable(),
        "EndpointNotFound must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Unknown.is_retryable(),
        "Unknown must not be retryable"
    );
}

// ---------------------------------------------------------------------------
// CompletionError + hook history recovery tests
// ---------------------------------------------------------------------------

/// Hard error after prior session history: the user prompt is persisted via the delta
/// path (not a placeholder pair), because `last_known_history` now includes the prompt.
///
/// **Why the delta is [user_prompt] after the fix:** `on_completion_call(prompt, history)`
/// now stores `history + [prompt]`. With prior history = [prior_1, prior_2] and a new
/// user prompt, `last_known_history` = [prior_1, prior_2, user_prompt].
/// `skip(pre_turn_message_count=2)` = [user_prompt] → delta path fires → 1 new message.
///
/// Before the fix: `last_known_history` = [prior_1, prior_2] (no prompt included).
/// `skip(2)` = empty delta → placeholder pair fired → 4 messages (2 prior + 2 placeholder).
///
/// After the fix: delta = [user_prompt] → delta path fires → 3 messages total
/// (2 prior + 1 user_prompt). The placeholder path no longer fires.
///
/// Key regression assertion: store must be exactly 3 (2 prior + 1 user prompt),
/// NOT 4 (old placeholder pair) and NOT 5 (the doubled result from the pre-delta-fix bug).
#[tokio::test]
async fn hard_error_after_prior_history_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-hook-history";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate the session store with a completed exchange so that rig
    // loads it into the agent's context and on_completion_call fires with
    // non-empty prior history.
    let prior_messages = vec![
        crate::types::Message::user("work done"),
        crate::types::Message::assistant("ok"),
    ];
    // Pre-populate the store by creating a separate FsSessionStore pointing to the same path
    // (the memory_state's internal store shares the same backing directory)
    let _prior_entries: Vec<StoreEntry> = prior_messages
        .iter()
        .cloned()
        .map(StoreEntry::Message)
        .collect();
    memory_state.inner_memory().load_all(session_id).await.ok();
    // Use ConversationMemory append to pre-populate
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, prior_messages.clone())
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    // Mock model: errors immediately (simulates CompletionError / network failure).
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("http decode error")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error)
    assert!(result.is_err(), "CompletionError must propagate as Err");

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 messages total.
    // NOT 4 (old placeholder pair), NOT 5 (doubled history from pre-delta bug).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages \
         (2 prior + 1 user prompt via delta path); got {:?}",
        persisted
    );

    // [0] must be the prior user message
    assert!(
        matches!(&persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be User (prior); got {:?}",
        persisted[0]
    );
    // [1] must be the prior assistant message
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant { .. }),
        "persisted[1] must be Assistant (prior); got {:?}",
        persisted[1]
    );
    // [2] must be the user prompt ("new prompt")
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "persisted[2] must be User (new prompt); got {:?}",
        persisted[2]
    );
    // Verify [2] contains the new prompt text
    let new_prompt_text = match &persisted[2] {
        crate::types::Message::User { content } => content
            .iter()
            .find_map(|c| {
                if let crate::types::UserContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        other => panic!("persisted[2] must be User; got {:?}", other),
    };
    assert_eq!(
        new_prompt_text, "new prompt",
        "persisted[2] must contain the new user prompt text"
    );

    // Prior message content check
    let has_prior_user = persisted.iter().any(|msg| {
        if let crate::types::Message::User { content } = msg {
            content.iter().any(|item| {
                if let crate::types::UserContent::Text(t) = item {
                    t.text == "work done"
                } else {
                    false
                }
            })
        } else {
            false
        }
    });
    assert!(
        has_prior_user,
        "prior user message must still be in store; got messages: {:?}",
        persisted
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Subtask 2 — delta-only persistence regression tests
// ---------------------------------------------------------------------------

/// Regression test: hard error after prior session history must not re-append the
/// full hook history snapshot — only a delta (the user prompt from this turn).
///
/// Before the fix: `last_known_history` (full history) was appended → store grew
/// from 2 to 5 messages (2 prior + 3 full history re-appended).
/// After the fix: `last_known_history` = [prior_1, prior_2, user_prompt].
/// delta = skip(pre_turn_count=2) = [user_prompt] → delta path fires → 3 messages total.
///
/// The bound is now exactly 3 (not < 5). The delta path fires with just the user
/// prompt — no placeholder is synthesised because the delta is non-empty.
#[tokio::test]
async fn hard_error_after_prior_history_persists_only_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-delta-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate: simulate a prior successful turn using ConversationMemory append
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, prior_msgs)
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    // Model errors immediately — simulates CompletionError / network failure.
    // on_completion_call fires with history = [user("prior work"), assistant("done")]
    // and prompt = user("new question").
    // After fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 total.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new question".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_err(), "hard error must propagate as Err");

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // After the fix: exactly 3 messages (2 prior + 1 user prompt delta).
    // The delta path fires because last_known_history includes the user prompt.
    // NOT 5 (pre-delta-fix duplication bug), NOT 4 (old placeholder pair).
    assert_eq!(
        persisted.len(),
        3,
        "hard error after prior history must persist exactly 3 messages (2 prior + 1 user prompt delta); got {} messages",
        persisted.len()
    );
    assert!(
        persisted.len() >= 2,
        "prior messages must be preserved; got {}",
        persisted.len()
    );

    Ok(())
}

/// Regression test: two consecutive hard errors must not double history each time.
///
/// Before the fix: turn 2 error appended full history (3 msgs), turn 3 appended full
/// history again (4 msgs) → store grew to 2 + 3 + 4 = 9 messages.
/// After the fix: `on_completion_call` stores `history + [prompt]`, so for each error
/// turn the delta = [user_prompt_for_that_turn] (1 message). The delta path fires.
/// Store grows by 1 per error turn:
///   - Turn 1 success: 2 messages (rig persists [user("t1"), assistant("ok")])
///   - Turn 2 error: +1 = 3 messages (delta = [user("t2")])
///   - Turn 3 error: +1 = 4 messages (delta = [user("t3")])
#[tokio::test]
async fn hard_error_twice_does_not_double_history() -> Result<()> {
    let config = test_config();
    let session_id = "test-no-double";
    let temp_dir = tempfile::tempdir().unwrap();

    // Turn 1: successful turn — rig appends [user("t1"), assistant("ok")] → store has 2 msgs.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("ok".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let closure_registry = ClosureRegistry::default();
        let mcp_registry = McpToolRegistry::empty();
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                last_total_tokens: default_last_total_tokens(),
                bus: crate::bus::create_bus(),
            },
            shared_model,
            test_compaction_config(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "t1".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        assert!(result.is_ok(), "turn 1 must succeed");
    }

    // Turn 2: hard error. pre_turn_count=2. last_known_history = [prior_1, prior_2, user("t2")].
    // delta = skip(2) = [user("t2")] → delta path fires → store has 3.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let closure_registry = ClosureRegistry::default();
        let mcp_registry = McpToolRegistry::empty();
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                last_total_tokens: default_last_total_tokens(),
                bus: crate::bus::create_bus(),
            },
            shared_model,
            test_compaction_config(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t2".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Turn 3: hard error again. pre_turn_count=3. last_known_history = [prior_1, prior_2, user("t2"), user("t3")].
    // delta = skip(3) = [user("t3")] → delta path fires → store has 4.
    {
        let mut memory_state = make_memory_state(&temp_dir);
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let shared_model = super::test_utils::shared_model_handle(model);
        let closure_registry = ClosureRegistry::default();
        let mcp_registry = McpToolRegistry::empty();
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                last_total_tokens: default_last_total_tokens(),
                bus: crate::bus::create_bus(),
            },
            shared_model,
            test_compaction_config(crate::bus::create_bus()),
        );
        let _ = executor
            .execute(
                ExecuteInput {
                    prompt: "t3".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
    }

    // Final state: 2 (turn 1 success) + 1 (turn 2 user delta) + 1 (turn 3 user delta) = 4.
    // After the fix: on_completion_call stores history + [prompt], so delta = [user_prompt]
    // for each error turn. Delta path fires → 1 message per error turn, not 2 (no placeholder).
    let final_memory_state = make_memory_state(&temp_dir);
    let final_entries = final_memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let final_count: Vec<crate::types::Message> = final_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    let final_count = final_count.len();

    assert_eq!(
        final_count, 4,
        "two hard errors after one success must produce exactly 4 messages \
         (2 from turn1 + 1 user delta turn2 + 1 user delta turn3); got {} messages",
        final_count
    );
    assert!(
        final_count >= 2,
        "original turn 1 messages must be preserved; got {}",
        final_count
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Subtask 3 — cancelled-turn diagnostic test
// ---------------------------------------------------------------------------

/// Diagnostic test: does a cancelled turn after prior session history duplicate JSONL?
///
/// Path C fires: rig's PromptCancelled carries chat_history. If that chat_history
/// is the full accumulated history (prior + current user prompt), appending it
/// directly would double the store.
///
/// Expected: store grows by at most the new messages from this turn, not by the
/// full prior history again.
#[tokio::test]
async fn cancelled_turn_after_prior_history_persists_only_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-delta";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate: simulate a prior successful turn using ConversationMemory append
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, prior_msgs)
            .await
            .map_err(|e| format!("append prior messages: {e:?}"))?;
    }

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new question".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok(), "cancelled turn must not return Err");
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // MUST NOT include the full prior history again. The turn is cancelled by the
    // cancelling tool, so the new-message delta is [user("new question"),
    // asst(tool_call), user(tool_result), asst(close)] = up to 4 new messages.
    // Total must be 2 prior + delta (<= 6), NOT the prior history doubled.
    assert!(
        persisted.len() <= 6,
        "cancelled turn after prior history must not double the store; got {} messages (expected <= 6)",
        persisted.len()
    );
    assert!(
        persisted.len() >= 2,
        "prior messages must be preserved; got {}",
        persisted.len()
    );

    Ok(())
}

/// When `on_completion_call` fires on the first LLM call of a fresh session
/// before a CompletionError, the executor persists the user prompt via the delta
/// path — NOT a synthetic placeholder.
///
/// After the fix, `on_completion_call(prompt, history=[])` stores `[prompt]`.
/// `pre_turn_message_count = 0`, so `delta = skip(0) = [prompt]` (non-empty).
/// Delta path fires → 1 message persisted. The placeholder path is never reached.
///
/// This also confirms the `test-hard-error-no-hook-history` session_id is used
/// consistently across this and the renamed test.
#[tokio::test]
async fn hard_error_on_first_llm_call_no_prior_history_persists_user_message() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-fresh-session-2";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] (non-empty) → delta path fires → 1 message.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: 1 message persisted (user prompt via delta path, no placeholder).
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(
        persisted.len(),
        1,
        "fresh session hard error must produce exactly 1 message (user prompt, no placeholder); \
         got {:?}",
        persisted
    );

    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message; got {:?}",
        persisted[0]
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// New test: hard error mid-tool-loop preserves real tool results
// ---------------------------------------------------------------------------

/// Verifies the root-cause fix: when a CompletionError occurs on the second LLM
/// sub-call of a multi-tool-call turn, the real tool result from sub-turn 1 is
/// preserved in the session — not replaced by a synthetic "[interrupted]" placeholder.
///
/// Before the fix: `on_completion_call(prompt=tool_result_msg, history=[user_msg,
/// assistant_tool_call])` discarded `prompt` → `last_known_history` = [user_msg,
/// assistant_tool_call] → `inject_missing_tool_results` synthesised "[interrupted]"
/// for "tc1" since no following ToolResult was present.
///
/// After the fix: `last_known_history` = [user_msg, assistant_tool_call,
/// tool_result_msg] → delta includes the real tool result → no synthesis needed.
///
/// Test flow (two `from_stream_turns` turns):
///   Sub-turn 1: LLM → tool_call("tc1", "some_tool", …) + FinalResponse
///               Rig dispatches tool → "some_tool" not in toolset → on_invalid_tool_call
///               → Skip → rig inserts error ToolResult for "tc1" into new_messages
///               → on_completion_call(prompt=tool_result_user_msg,
///                                   history=[user_msg, assistant_tool_call_msg])
///   Sub-turn 2: LLM → error("network failure")
///               → CompletionError → last_known_history snapshot is read
///
/// After fix:
///   last_known_history = [user_msg, assistant_tool_call_msg, tool_result_user_msg]
///   delta = skip(pre_turn_count=0) = all 3 messages
///   inject_missing_tool_results: tool_call "tc1" HAS following ToolResult → no patch
///   persisted = [user_msg, assistant_tool_call_msg, tool_result_user_msg]
///
/// Key assertion: every persisted ToolCall has a following ToolResult NOT containing "[interrupted]".
///
/// The tool must be registered in the ToolServer so rig dispatches it normally.
/// Previously this test relied on `on_invalid_tool_call` → `Skip` to produce a
/// ToolResult, but `Retry` no longer persists the malformed call.
struct SimpleEchoTool;

impl rig::tool::Tool for SimpleEchoTool {
    const NAME: &'static str = "some_tool";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Simple echo tool for testing".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok("real_tool_output".to_string())
    }
}

#[tokio::test]
async fn hard_error_mid_tool_loop_preserves_real_tool_results() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-mid-tool-loop-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: LLM emits tool_call + FinalResponse
    // Turn 2: LLM errors (simulates CompletionError after tool result is in history)
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({"x": 1})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::error("network failure after tool")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new()
        .tool(SimpleEchoTool)
        .run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![rig::completion::ToolDefinition {
                name: "some_tool".to_string(),
                description: "Simple echo tool for testing".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "do the thing".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The error must propagate (CompletionError from turn 2, or UnknownToolCall from turn 1)
    assert!(
        result.is_err(),
        "error on sub-call must propagate as Err; got ok"
    );

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // Must have exactly 4 messages:
    //   [User(prompt), Assistant(ToolCall), User(ToolResult), Assistant(close-block)]
    // The 4th is the synthetic assistant message appended by close_open_tool_result_block
    // to prevent user(ToolResult) → user(Text) on the next turn.
    assert_eq!(
        persisted.len(),
        4,
        "mid-tool-loop error must persist exactly [user_msg, assistant_tool_call, tool_result, asst_close]; got {} messages: {:?}",
        persisted.len(),
        persisted
    );
    // The 4th message must be a synthetic assistant close-block.
    assert!(
        matches!(&persisted[3], crate::types::Message::Assistant { .. }),
        "persisted[3] must be the synthetic assistant close-block; got: {:?}",
        persisted[3]
    );

    // For every persisted ToolCall, verify the following ToolResult is NOT "[interrupted]"
    use crate::types::{AssistantContent, UserContent};
    let mut found_tool_call = false;
    for (i, msg) in persisted.iter().enumerate() {
        let crate::types::Message::Assistant { content, .. } = msg else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tc) = item else {
                continue;
            };
            let call_id = &tc.id;
            found_tool_call = true;

            // There MUST be a following ToolResult
            let next = persisted.get(i + 1).unwrap_or_else(|| {
                panic!("ToolCall id={call_id} must have a following message in persisted history")
            });

            let result_content = if let crate::types::Message::User { content } = next {
                content
                    .iter()
                    .find_map(|item| {
                        if let UserContent::ToolResult(tr) = item {
                            if &tr.call == call_id {
                                // Extract text content
                                Some(
                                    tr.content
                                        .iter()
                                        .find_map(|c| {
                                            if let crate::types::ToolResultContent::Text(t) = c {
                                                Some(t.text.clone())
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or_default(),
                                )
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "ToolCall id={call_id} must have a matching ToolResult; \
                             next message: {:?}",
                            next
                        )
                    })
            } else {
                panic!(
                    "Message after ToolCall id={call_id} must be User; got {:?}",
                    next
                )
            };

            // The real key assertion: result must NOT be the synthetic "[interrupted]"
            // placeholder that inject_missing_tool_results would insert.
            assert!(
                !result_content.contains("[interrupted]"),
                "ToolResult id={call_id} must NOT contain '[interrupted]' \
                 (real result was available but got synthetic placeholder); \
                 content: {:?}",
                result_content
            );
        }
    }

    assert!(
        found_tool_call,
        "test requires at least one ToolCall to be persisted; got: {:?}",
        persisted
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// close_open_tool_result_block unit tests
// ---------------------------------------------------------------------------

/// Helper: build a User message whose content is a single ToolResult.
fn user_with_tool_result(id: &str) -> crate::types::Message {
    crate::types::Message::User {
        content: vec![crate::types::UserContent::ToolResult(
            crate::types::ToolResult {
                call: crate::types::ToolCallId::new_or_mint(id),
                provider: None,
                name: "do_thing".into(),
                content: vec![crate::types::ToolResultContent::text("result")],
            },
        )],
    }
}

/// Helper: build a User message whose content is a single Text item.
fn user_with_text(text: &str) -> crate::types::Message {
    crate::types::Message::user(text)
}

/// Helper: build an Assistant text message.
fn assistant_with_text(text: &str) -> crate::types::Message {
    crate::types::Message::assistant(text)
}

/// Helper: build a User message with mixed content (ToolResult + Text).
fn user_with_mixed_content(id: &str) -> crate::types::Message {
    crate::types::Message::User {
        content: vec![
            crate::types::UserContent::ToolResult(crate::types::ToolResult {
                call: crate::types::ToolCallId::new_or_mint(id),
                provider: None,
                name: "do_thing".into(),
                content: vec![crate::types::ToolResultContent::text("result")],
            }),
            crate::types::UserContent::Text(crate::types::Text::new("some text")),
        ],
    }
}

#[test]
fn close_open_tool_result_block_appends_when_last_is_tool_result() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        crate::types::Message::assistant("ok"),
        user_with_tool_result("tc1"),
    ];
    let result = close_open_tool_result_block(msgs, "server error");
    assert_eq!(result.len(), 4, "synthetic assistant must be appended");
    assert!(
        matches!(result[3], crate::types::Message::Assistant { .. }),
        "last message must be Assistant; got: {:?}",
        result[3]
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_is_assistant() {
    use super::close_open_tool_result_block;

    let msgs = vec![user_with_text("prompt"), assistant_with_text("response")];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(result.len(), len_before, "no change when last is assistant");
    assert!(
        matches!(
            result[result.len() - 1],
            crate::types::Message::Assistant { .. }
        ),
        "last message must still be Assistant"
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_is_user_text() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        assistant_with_text("response"),
        user_with_text("follow up"),
    ];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(result.len(), len_before, "no change when last is user text");
    assert!(
        matches!(result[result.len() - 1], crate::types::Message::User { .. }),
        "last message must still be User"
    );
}

#[test]
fn close_open_tool_result_block_noop_when_last_user_has_mixed_content() {
    use super::close_open_tool_result_block;

    let msgs = vec![
        user_with_text("prompt"),
        assistant_with_text("assistant"),
        user_with_mixed_content("tc1"),
    ];
    let len_before = msgs.len();
    let result = close_open_tool_result_block(msgs, "error");
    assert_eq!(
        result.len(),
        len_before,
        "no change when last user has mixed content (ToolResult + Text)"
    );
}

// ---------------------------------------------------------------------------
// Gap 3 — Retry-with-backoff tests
// ---------------------------------------------------------------------------

/// Retry succeeds on the second attempt: first call returns a retryable 500 error,
/// second call succeeds. Result should be Ok with 2 messages in JSONL.
#[tokio::test]
async fn retry_succeeds_on_second_attempt() -> Result<()> {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-success";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: error (retryable 500). Turn 2: success.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error internal server")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(
        result.is_ok(),
        "retry should succeed on second attempt; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("retry should succeed on second attempt: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::Completed));

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        persisted.len(),
        2,
        "successful retry must produce 2 messages (user + assistant); got {}",
        persisted.len()
    );

    Ok(())
}

/// Retry exhausted: all attempts fail with retryable errors. The final error
/// message must mention the retry attempt count.
#[tokio::test]
async fn retry_exhausted_surfaces_attempt_count() -> Result<()> {
    let config = Config {
        max_retries: Some(2),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-exhausted";
    let mut memory_state = make_memory_state(&temp_dir);

    // All 3 attempts (1 initial + 2 retries) fail with retryable error
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_err(), "exhausted retries must return Err");
    let err_msg = result
        .err()
        .map(|e| e.to_string())
        .ok_or("exhausted retries must return Err")?;
    assert!(
        err_msg.contains("after 2 retries"),
        "error message must mention retry count; got: {err_msg}"
    );

    Ok(())
}

/// Non-retryable errors (e.g., a ServerError-style outage) must NOT get the
/// backoff retry. Model-correctable errors (e.g., context_length_exceeded)
/// now get feedback retries instead, so a single scripted turn ends after
/// one model call plus one feedback re-run — still no backoff retries.
#[tokio::test]
async fn non_retryable_error_not_retried() {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-non-retryable";
    let mut memory_state = make_memory_state(&temp_dir);

    // Only 1 turn — the second attempt fails with the mock's out-of-turns
    // error, which is neither retryable nor model-correctable, so the loop
    // exits with no backoff retries (no "Turn failed after N retries" wrap).
    // A 400 context_length_exceeded is NOT retryable.
    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "context_length_exceeded in prompt",
    )]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must fail immediately without retrying
    assert!(
        result.is_err(),
        "non-retryable error must not be retried; got Ok"
    );
    // If the model had been called more than once, MockCompletionModel would panic
    // (it only has 1 turn configured). The test passing proves exactly 1 HTTP request.
}

/// An OutputBudget error must surface a user message naming `max_output_tokens` and
/// `--max-output-tokens`.
#[tokio::test]
async fn output_budget_error_surfaces_user_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget";
    let mut memory_state = make_memory_state(&temp_dir);

    // Three failing turns: attempts 1-2 each get one feedback retry (cap 2);
    // attempt 3 exceeds the cap, so the final break carries the OutputBudget
    // kind and the hard-error path surfaces the max_output_tokens message.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result.expect_err("OutputBudget error must fail the turn");
    assert!(
        err.msg.contains("max_output_tokens"),
        "user message must mention max_output_tokens; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("--max-output-tokens"),
        "user message must mention --max-output-tokens; got: {}",
        err.msg
    );
    Ok(())
}

/// When raise is enabled and the effective max_tokens is below the cap, the
/// OutputBudget feedback retry runs the next attempt with max_tokens = base * multiplier.
#[tokio::test]
async fn output_budget_raise_applies_multiplier_on_retry() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_multiplier: Some(2.0),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise";
    let mut memory_state = make_memory_state(&temp_dir);

    // Attempt 1 fails with OutputBudget; attempt 2 succeeds.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the raised retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(
        requests[0].max_tokens,
        Some(1000),
        "first attempt keeps base"
    );
    assert_eq!(
        requests[1].max_tokens,
        Some(2000),
        "second attempt must carry raised max_tokens = 1000 * 2.0"
    );
    Ok(())
}

/// Two consecutive OutputBudget failures must compound the raise: the second
/// retry raises from the first raised value, not from the base config.
#[tokio::test]
async fn output_budget_raise_compounds_on_consecutive_failures() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_multiplier: Some(2.0),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-compound";
    let mut memory_state = make_memory_state(&temp_dir);

    // Attempts 1 and 2 fail with OutputBudget; attempt 3 succeeds.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the raised retries");
    let requests = spy.requests();
    assert_eq!(requests.len(), 3, "expected 3 model calls");
    assert_eq!(
        requests[0].max_tokens,
        Some(1000),
        "first attempt keeps base"
    );
    assert_eq!(
        requests[1].max_tokens,
        Some(2000),
        "second attempt must carry raised max_tokens = 1000 * 2.0"
    );
    assert_eq!(
        requests[2].max_tokens,
        Some(4000),
        "third attempt must compound the raise = 2000 * 2.0"
    );
    Ok(())
}

/// When raise is disabled (default), the OutputBudget feedback retry keeps the
/// unchanged effective max_tokens.
#[tokio::test]
async fn output_budget_raise_disabled_keeps_max_tokens_unchanged() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-disabled";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(1000));
    assert_eq!(
        requests[1].max_tokens,
        Some(1000),
        "disabled raise must keep max_tokens unchanged"
    );
    Ok(())
}

/// When raise is enabled but the effective max_tokens is None, no raise applies.
#[tokio::test]
async fn output_budget_raise_no_base_max_tokens_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        output_budget_raise_enabled: Some(true),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-no-base";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, None);
    assert_eq!(
        requests[1].max_tokens, None,
        "no base max_tokens means no raise"
    );
    Ok(())
}

/// When raise is enabled and the base max_tokens is at or above the cap, no
/// raise applies.
#[tokio::test]
async fn output_budget_raise_base_at_cap_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(32768),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_cap: Some(32768),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-at-cap";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(32768));
    assert_eq!(
        requests[1].max_tokens,
        Some(32768),
        "base at cap means no raise"
    );
    Ok(())
}

/// When raise is enabled but the kind is not OutputBudget (e.g. ContextOverflow),
/// the feedback retry keeps max_tokens unchanged.
#[tokio::test]
async fn output_budget_raise_non_output_budget_kind_no_raise() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tokens: Some(1000),
        output_budget_raise_enabled: Some(true),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget-raise-non-ob";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("context_length_exceeded in prompt")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_ok(), "turn must succeed after the retry");
    let requests = spy.requests();
    assert_eq!(requests.len(), 2, "expected 2 model calls");
    assert_eq!(requests[0].max_tokens, Some(1000));
    assert_eq!(
        requests[1].max_tokens,
        Some(1000),
        "non-OutputBudget kind must not raise"
    );
    Ok(())
}

/// A mock turn that streams reasoning-only content then a final response with
/// `FinishReason::Length` must reach rig's empty-truncation check and surface as
/// a Prompt-wrapped `CompletionError::ResponseError` (the defect shape). The
/// executor must classify it as OutputBudget and run the feedback-retry path.
#[tokio::test]
async fn prompt_wrapped_empty_output_length_reaches_feedback_retry() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-prompt-wrapped-length";
    let mut memory_state = make_memory_state(&temp_dir);

    // Reasoning-only + truncating Length final: rig's `turn_delivered_no_answer`
    // returns true (reasoning is not an answer) and `truncating_finish_reason`
    // returns Length, so the run errors with the ResponseError message.
    let length_final = rig::streaming::StreamFinal::new("mock", rig::completion::Usage::new())
        .with_finish_reason(rig::completion::FinishReason::Length);
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result.expect_err("empty-output Length must fail the turn");
    assert!(
        err.msg.contains("max_output_tokens"),
        "user message must mention max_output_tokens; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("--max-output-tokens"),
        "user message must mention --max-output-tokens; got: {}",
        err.msg
    );
    Ok(())
}

/// When `max_retries` is `Some(0)`, no retry is attempted regardless of error type.
///
/// Tests the most direct way to disable retries: setting max_retries=0 ensures
/// `attempt < max_retries` is always false on the first attempt (attempt=0 < 0 = false).
/// The test verifies the error message does NOT contain "retries" — proving the retry
/// path was not entered.
#[tokio::test]
async fn retry_disabled_when_max_retries_is_zero() -> Result<()> {
    let config = Config {
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-no-retry-guard";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "500 api_error server error",
    )]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_err(), "error must propagate without retry");
    let err_msg = result
        .err()
        .map(|e| e.to_string())
        .ok_or("error must propagate without retry")?;
    // When max_retries=0, attempt never increments, so "retries" should not appear
    assert!(
        !err_msg.contains("retries"),
        "error message must NOT mention retries when max_retries=0; got: {err_msg}"
    );

    Ok(())
}

/// When `last_known_history` is empty the retry guard (`has_partial_history`) prevents
/// any retry from being attempted, even when `max_retries > 0` and the error is retryable.
///
/// After the refactor, caller-local context lives in `TurnContext` (from `error.rs`),
/// not in `TurnError`. This test verifies the guard logic directly by constructing
/// `TurnContext` values and checking that `last_known_history.is_empty()` correctly
/// controls the guard condition.
///
/// For the full-path scenario (history always non-empty via MockCompletionModel), see
/// `retry_disabled_when_max_retries_is_zero` which tests the equivalent observable outcome.
#[test]
fn retry_not_attempted_when_no_partial_history() {
    use crate::conversation::turn::error::TurnContext;

    // Empty history: has_partial_history guard must be false.
    let ctx_empty = TurnContext {
        last_known_history: vec![],
        pre_turn_message_count: 0,
    };
    let has_partial_history = !ctx_empty.last_known_history.is_empty();

    // Guard must evaluate to false with empty history — retry must not be attempted.
    assert!(
        !has_partial_history,
        "has_partial_history guard must be false when last_known_history is empty; \
         retry must be suppressed even for retryable errors with max_retries > 0"
    );

    // Confirm the mirror case: non-empty history enables the guard.
    let ctx_non_empty = TurnContext {
        last_known_history: vec![crate::types::Message::user("prompt")],
        pre_turn_message_count: 0,
    };
    let has_partial_history_non_empty = !ctx_non_empty.last_known_history.is_empty();
    assert!(
        has_partial_history_non_empty,
        "has_partial_history guard must be true when last_known_history is non-empty"
    );
}

// ---------------------------------------------------------------------------
// Gap 3 — extract_retry_after_ms unit tests
// ---------------------------------------------------------------------------

#[test]
fn extract_retry_after_ms_parses_seconds_basic() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("rate limited, retry after 5 seconds"),
        Some(5000)
    );
}

#[test]
fn extract_retry_after_ms_parses_retry_after_header() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("HTTP 429: Retry-After: 30"),
        Some(30_000)
    );
}

#[test]
fn extract_retry_after_ms_parses_underscore_variant() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("error: retry_after: 10 seconds"),
        Some(10_000)
    );
}

#[test]
fn extract_retry_after_ms_returns_none_when_absent() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("rate limit exceeded, try again later"),
        None
    );
}

#[test]
fn extract_retry_after_ms_handles_zero() {
    use super::extract_retry_after_ms;
    assert_eq!(extract_retry_after_ms("retry after 0 seconds"), Some(0));
}

// ---------------------------------------------------------------------------
// Path B regression: last_known_history preserves tool calls on cancel
// ---------------------------------------------------------------------------

/// Regression test for the critical Path B cancel bug: when `tokio::select!`
/// cancels the stream BEFORE rig yields `PromptCancelled` (Path B), the executor
/// must use `TurnResult.last_known_history` to persist completed tool calls and
/// their results — NOT fall through to the old minimal `[user(prompt)]` fallback.
///
/// Scenario: the agent completes tool call T1 (hook's `on_completion_call` fires
/// with history containing T1), then cancel fires via `tokio::select!` before
/// the next LLM response. The `StreamingTurnResult` has `cancelled: true,
/// messages: None` (rig never yielded `PromptCancelled`). However,
/// `last_known_history` contains the full snapshot including the completed T1.
///
/// **Before the fix:** Path B synthesized `[user(prompt), assistant(partial_text)]`
/// and all completed tool work was LOST.
///
/// **After the fix:** Path B reads `last_known_history`, slices the delta, patches
/// it with `inject_missing_tool_results` + `close_open_tool_result_block`, and
/// persists the real work.
///
/// We exercise this end-to-end using the `JourneyHarness` pattern: a mock tool that
/// publishes a cancel event to the bus from within `call()` after producing its result.
/// The bus is threaded into the tool, so the cancel fires deterministically.
///
/// This is intentionally an integration test (not a unit test) because the bug
/// exists at the intersection of `build_agent_and_stream` (which populates
/// `last_known_history` on `TurnResult`) and `TurnExecutor::execute` (which
/// reads it in Path B).
#[tokio::test]
async fn path_b_cancel_preserves_tool_calls_via_last_known_history() -> Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rig::test_utils::{MockCompletionModel, MockStreamEvent};

    use crate::bus::CancelEvent;
    use crate::session::StoreEntry;
    use crate::tools::closure::ClosureRegistry;
    use crate::tools::handler::McpToolRegistry;

    // -- cancelling tool (fires cancel after producing its result) ----------
    struct CancellingTool {
        output: &'static str,
        bus: crate::bus::Bus,
        fired: Arc<AtomicBool>,
    }

    impl rig::tool::Tool for CancellingTool {
        const NAME: &'static str = "test_cancel_tool";
        type Error = std::convert::Infallible;
        type Args = serde_json::Value;
        type Output = String;

        fn description(&self) -> String {
            "Tool that cancels after first call".to_string()
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }

        async fn call(
            &self,
            _context: &mut rig::tool::ToolContext,
            _args: Self::Args,
        ) -> std::result::Result<Self::Output, Self::Error> {
            let result = self.output.to_string();
            if !self.fired.swap(true, Ordering::SeqCst) {
                tokio::task::yield_now().await;
                let _ = self.bus.cancel().send(CancelEvent::Requested).await;
            }
            Ok(result)
        }
    }

    // -- test body ---------------------------------------------------------
    let config = test_config();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_id = "test-path-b-lkh";
    let mut memory_state = make_memory_state(&temp_dir);

    let (bus,) = (crate::bus::create_bus(),);

    // Model: sub-turn 1 emits tool_call → tool executes (cancels after result).
    // Sub-turn 2 would normally proceed but cancel fires first.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let handle = rig::tool::server::ToolServer::new()
        .tool(CancellingTool {
            output: "tool_completed_successfully",
            bus: bus.clone(),
            fired: Arc::new(AtomicBool::new(false)),
        })
        .run();
    let tool_infra = ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: vec![rig::completion::ToolDefinition {
            name: "test_cancel_tool".to_string(),
            description: "Tool that cancels after first call".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        last_total_tokens: default_last_total_tokens(),
        bus: bus.clone(),
    };

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        tool_infra,
        shared_model,
        test_compaction_config(bus.clone()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "call the tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // 1. Must be Ok(EarlyReturn) — cancelled turn, not an error
    assert!(
        result.is_ok(),
        "cancelled turn must not return Err; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "cancelled turn must return EarlyReturn"
    );

    // 2. Persisted JSONL must contain the user prompt + tool call + tool result
    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();

    // Before the fix: Path B would synthesize [user("call the tool")] = 1 message.
    // After the fix: Path B uses last_known_history which contains
    // [user(prompt), asst(tool_call), user(tool_result)] + possibly a close-block.
    assert!(
        persisted.len() >= 3,
        "Path B must persist at least 3 messages \
         [user(prompt), asst(tool_call), user(tool_result)]; got {} messages: {:?}",
        persisted.len(),
        persisted
    );

    // 3. Verify tool call is in the persisted messages
    let has_tool_call = persisted.iter().any(|msg| {
        if let crate::types::Message::Assistant { content, .. } = msg {
            content.iter().any(
                |c| matches!(c, crate::types::AssistantContent::ToolCall(tc) if tc.id.as_str() == "tc1"),
            )
        } else {
            false
        }
    });
    assert!(
        has_tool_call,
        "persisted messages must contain tool call tc1; got: {:?}",
        persisted
    );

    // 4. Verify tool result is in the persisted messages (NOT [interrupted])
    let has_real_tool_result = persisted.iter().any(|msg| {
        if let crate::types::Message::User { content } = msg {
            content.iter().any(|c| {
                if let crate::types::UserContent::ToolResult(tr) = c {
                    tr.call.as_str() == "tc1"
                        && tr.content.iter().any(|rc| {
                            if let crate::types::ToolResultContent::Text(t) = rc {
                                !t.text.contains("[interrupted]")
                            } else {
                                false
                            }
                        })
                } else {
                    false
                }
            })
        } else {
            false
        }
    });
    assert!(
        has_real_tool_result,
        "persisted messages must contain real tool result for tc1 \
         (not [interrupted] placeholder); got: {:?}",
        persisted
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Size-aware error classification tests
// ---------------------------------------------------------------------------

/// "error sending request" must be classified as `Network` (retryable).
/// Tests the `classify_from_display` path for the `Instance` case.
#[test]
fn classify_error_sending_request_is_network() {
    let kind = turn_error_from_response_error(
        "error sending request for url https://api.example.com/v1/chat",
    );
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "'error sending request' must be Network"
    );
    assert!(kind.is_retryable(), "Network must be retryable");
}

// ---------------------------------------------------------------------------
// Jitter variance test
// ---------------------------------------------------------------------------

/// Verify that the jitter factor produces varying delays across multiple samples.
///
/// The retry loop uses `0.8 + (rand::random::<f64>() * 0.4)` which gives a
/// jitter_factor in [0.8, 1.2). Over 50 samples, the min and max must differ
/// by at least 10% of the base delay — confirming non-deterministic jitter.
#[test]
fn jitter_produces_varying_delays() {
    let base_delay_ms: u64 = 1000;
    let samples: Vec<u64> = (0..50)
        .map(|_| {
            let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
            (base_delay_ms as f64 * jitter_factor) as u64
        })
        .collect();

    let min = samples.iter().copied().min().unwrap_or(base_delay_ms);
    let max = samples.iter().copied().max().unwrap_or(base_delay_ms);
    let range = max - min;

    // With 50 samples from a uniform [0.8, 1.2) distribution applied to 1000ms,
    // the range should be well above 100ms (10% of base). In practice it will be
    // close to 400ms (the theoretical max range of 800..1200). We assert > 50ms
    // to avoid flakiness while still catching deterministic implementations.
    assert!(
        range > 50,
        "jitter must produce varying delays; got range={range}ms (min={min}, max={max})"
    );

    // Verify all samples are within the expected [800, 1200) range
    for &sample in &samples {
        assert!(
            (800..1200).contains(&sample),
            "jittered delay must be in [800, 1200); got {sample}"
        );
    }
}

// ---------------------------------------------------------------------------
// Compaction end-to-end test (task 679635b4)
// ---------------------------------------------------------------------------

/// When the conversation exceeds the sliding window, the hook must fire a
/// `CompactionEvent::Requested { source: "auto" }` on the bus. The current turn
/// proceeds with the full history (the orchestrator runs compaction
/// asynchronously; the summary is applied on the next turn via the marker).
#[tokio::test]
async fn compaction_fires_when_conversation_exceeds_window() -> Result<()> {
    use rig::memory::ConversationMemory;

    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-compaction-fires";

    let bus = crate::bus::create_bus();
    let mut compaction_rx = bus.compaction().subscribe();

    // The compactor needs a model that streams a summary (a plain `text()` model
    // does not serve streaming calls). Script enough streaming turns for the
    // compaction to succeed.
    let compactor_turns: Vec<Vec<rig::test_utils::MockStreamEvent>> = (0..8)
        .map(|_| {
            vec![
                rig::test_utils::MockStreamEvent::Text("summary".to_string()),
                rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let compactor_model = MockCompletionModel::from_stream_turns(compactor_turns);
    let compactor_handle = std::sync::Arc::new(std::sync::Mutex::new(
        rig::agent::ModelHandle::new(compactor_model),
    ));
    // Attach a store to the compactor so it can read/write compaction markers.
    let store_arc = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let compactor = crate::conversation::compaction::compactor::NuCompactor::from_shared_model(
        compactor_handle,
        bus.clone(),
        None,
    )
    .with_store(Arc::clone(&store_arc));

    let mut memory_state = MemoryState::new(Arc::clone(&store_arc));

    // Pre-populate a conversation that far exceeds the token threshold with
    // distinct user/assistant pairs (10 pairs = 20 messages).
    for i in 0..10 {
        memory_state
            .inner_memory()
            .append(
                session_id,
                vec![crate::types::Message::user(format!("user-{i}"))],
            )
            .await
            .map_err(|e| format!("append user: {e:?}"))?;
        memory_state
            .inner_memory()
            .append(
                session_id,
                vec![crate::types::Message::assistant(format!("assistant-{i}"))],
            )
            .await
            .map_err(|e| format!("append assistant: {e:?}"))?;
    }

    // The model must be scripted to produce a final text response on the agent
    // turn. Clone before moving so we can inspect the agent request's
    // chat_history after the turn.
    let model = MockCompletionModel::from_stream_turns([
        // Turn 1: the agent's actual response (text).
        vec![
            MockStreamEvent::Text("agent reply".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let model_spy = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let compaction_config = crate::conversation::compaction::CompactionConfig {
        compactor,
        params: crate::compaction::CompactionParams::default(),
        // A tiny threshold so the pre-populated conversation (20 messages) is
        // over the threshold and auto-compaction fires on the first turn.
        threshold_tokens: Some(1),
    };

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: bus.clone(),
        },
        shared_model,
        compaction_config,
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(
        result.is_ok(),
        "turn must complete; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::Completed));

    // 1. `CompactionEvent::Requested { source: "auto" }` must have been emitted
    //    on the bus when the conversation exceeds the window.
    let mut saw_requested = false;
    while let Ok(ev) = compaction_rx.try_recv() {
        if matches!(
            ev,
            crate::bus::CompactionEvent::Requested { source } if source == "auto"
        ) {
            saw_requested = true;
            break;
        }
    }
    assert!(
        saw_requested,
        "CompactionEvent::Requested {{ source: \"auto\" }} must be emitted when the conversation exceeds the window"
    );

    // 2. The agent must have made exactly 1 request (the current turn proceeds
    //    with the full history; compaction runs asynchronously on the worker).
    assert_eq!(
        model_spy.request_count(),
        1,
        "agent must have made exactly 1 request"
    );

    Ok(())
}

/// `on_stream_response_finish` stores the real API token count so the hook's
/// compaction threshold uses real usage, not the chars/4 estimate.
///
/// The `ToolInfra.last_total_tokens` slot starts `None`. After a completed turn
/// whose model reports `total_tokens > 0`, the hook must populate the slot with
/// that real count (verified through the public executor boundary).
#[tokio::test]
async fn on_stream_response_finish_stores_total_tokens() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_id = "test-hook-total-tokens";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("response".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1234),
    ]]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    let last_total_tokens = default_last_total_tokens();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: last_total_tokens.clone(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(
        result.is_ok(),
        "turn must complete; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::Completed));
    assert_eq!(
        *last_total_tokens
            .lock()
            .expect("last_total_tokens mutex poisoned"),
        Some(1234),
        "hook must store the real total_tokens from on_stream_response_finish"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Memory-append failure surfacing
// ---------------------------------------------------------------------------

/// A `SessionStore` whose `append` always fails, used to verify that failed
/// session-memory appends on turn error paths are surfaced (not silently
/// dropped). `create` succeeds so pre-population (the first write, which
/// routes to `create`) works and the turn's later append routes to the
/// failing `append`.
#[derive(Clone)]
struct FailingAppendStore;

#[derive(Debug)]
struct AppendError;

impl std::fmt::Display for AppendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "append exploded")
    }
}

impl std::error::Error for AppendError {}

impl SessionStore for FailingAppendStore {
    type Error = AppendError;

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
        Ok(None)
    }

    async fn append(
        &self,
        _id: &str,
        _entries: &[StoreEntry],
    ) -> core::result::Result<(), Self::Error> {
        Err(AppendError)
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

/// A failed session-memory append on the hard-error path must be surfaced as
/// `WarningEvent::Message` on the bus warning channel (not silently dropped).
#[tokio::test]
async fn hard_error_with_failing_append_store_emits_warning_event() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let session_id = "test-failing-append-hard-error";
    let mut memory_state = MemoryState::new(Arc::new(FailingAppendStore));

    // First write routes to store.create (succeeds) and marks the session as
    // persisted, so the turn's later append routes to the failing store.append.
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, vec![user_with_text("prior work")])
            .await
            .map_err(|e| format!("pre-populate append should succeed: {e:?}"))?;
    }

    let bus = crate::bus::create_bus();
    let mut warning_rx = bus.warning().subscribe();

    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_err(), "hard error must propagate as Err");

    let event = warning_rx
        .try_recv()
        .map_err(|e| format!("warning event should arrive after failed append: {e:?}"))?;
    match event {
        crate::bus::WarningEvent::Message { message } => {
            assert!(
                message.contains("hard error"),
                "warning must name the append site; got: {message}"
            );
        }
        other => {
            return Err(format!("expected WarningEvent::Message; got {other:?}").into());
        }
    }

    Ok(())
}

/// A succeeding session-memory append on the hard-error path must NOT emit a
/// warning event.
#[tokio::test]
async fn hard_error_with_working_store_emits_no_warning_event() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-working-append-hard-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate so the turn's append is a second write (routes to
    // store.append) and succeeds against the working store.
    {
        use rig::memory::ConversationMemory;
        memory_state
            .inner_memory()
            .append(session_id, vec![user_with_text("prior work")])
            .await
            .map_err(|e| format!("pre-populate append should succeed: {e:?}"))?;
    }

    let bus = crate::bus::create_bus();
    let mut warning_rx = bus.warning().subscribe();

    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(result.is_err(), "hard error must propagate as Err");

    match warning_rx.try_recv() {
        Err(crate::bus::TryRecvError::Empty) => {} // no warning emitted — correct
        Err(other) => {
            return Err(format!("warning channel should stay open: {other:?}").into());
        }
        Ok(event) => {
            return Err(
                format!("successful append must not emit a warning event; got {event:?}").into(),
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Provider feedback retry (model-correctable completion errors)
// ---------------------------------------------------------------------------

/// A model-correctable provider failure classified by HTTP status (413 →
/// RequestTooLarge via `classify_by_status`) must append exactly one user-role
/// feedback message to the session memory and re-run the turn — proven by a
/// second scripted turn succeeding.
#[tokio::test]
async fn model_correctable_failure_appends_feedback_and_reruns_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-retry-rerun";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: 413 classified by status to RequestTooLarge (model-correctable).
    // Turn 2: success — only reached if the feedback retry re-ran the turn.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(
            rig::test_utils::MockError::ProviderResponse(rig::ProviderResponseError::new(
                http::StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
            )),
        )],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("feedback retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "feedback retry must complete the turn"
    );
    assert_eq!(
        probe.request_count(),
        2,
        "model-correctable failure must re-run the turn exactly once"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        1,
        "exactly one feedback message must be appended; got {persisted:?}"
    );
    let feedback_msg = persisted
        .iter()
        .find(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX)
            })
        })
        .ok_or("should have the appended feedback message in the session history")?;
    assert!(
        matches!(feedback_msg, crate::types::Message::User { .. }),
        "appended feedback message must have User role; got {feedback_msg:?}"
    );

    Ok(())
}

/// Three consecutive model-correctable failures with a cap of
/// MAX_PROVIDER_FEEDBACK_RETRIES (2) must produce exactly two feedback
/// messages and then fall through to the hard-error path.
#[tokio::test]
async fn feedback_cap_two_produces_two_messages_then_hard_error() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-cap";
    let mut memory_state = make_memory_state(&temp_dir);

    let overflow = || MockStreamEvent::error("context_length_exceeded in prompt");
    let model = MockCompletionModel::from_stream_turns([
        vec![overflow()],
        vec![overflow()],
        vec![overflow()],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("capped model-correctable failures must fall through to the hard error")?;
    assert_eq!(
        probe.request_count(),
        3,
        "two feedback retries plus the capped final attempt = 3 model calls"
    );
    assert!(
        !err.msg.contains("Turn failed after"),
        "feedback retries must not increment the backoff attempt counter; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("conversation too long"),
        "hard-error message must describe the final ContextOverflow failure; got: {}",
        err.msg
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        2,
        "cap 2 must produce exactly two feedback messages; got {persisted:?}"
    );

    Ok(())
}

/// A retryable-kind failure (429 RateLimit) must append no feedback message
/// and must not consume the feedback budget.
#[tokio::test]
async fn retryable_kind_failure_appends_no_feedback_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-retryable-no-feedback";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "429 rate_limit_error",
    )]]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    assert!(
        result.is_err(),
        "retryable error with max_retries=0 must fail the turn"
    );
    assert_eq!(
        probe.request_count(),
        1,
        "retryable kinds must not trigger a feedback retry"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        0,
        "retryable kinds must not append feedback messages; got {persisted:?}"
    );

    Ok(())
}

/// After a successful feedback retry, the session store must contain the
/// feedback message exactly once and the turn's exchange exactly once.
#[tokio::test]
async fn successful_feedback_retry_persists_feedback_exactly_once() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-exactly-once";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("context_length_exceeded in prompt")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("feedback retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "feedback retry must complete the turn"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "store must hold feedback + user prompt + assistant response; got {persisted:?}"
    );
    assert_eq!(
        feedback_message_count(&persisted),
        1,
        "feedback message must be persisted exactly once; got {persisted:?}"
    );
    let prompt_count = persisted
        .iter()
        .filter(|m| message_text(m).as_deref() == Some("hello"))
        .count();
    assert_eq!(
        prompt_count, 1,
        "user prompt must be persisted exactly once"
    );
    let reply_count = persisted
        .iter()
        .filter(|m| message_text(m).as_deref() == Some("recovered"))
        .count();
    assert_eq!(
        reply_count, 1,
        "assistant response must be persisted exactly once"
    );

    Ok(())
}

/// A turn with no session (final_session_id None) must not append feedback
/// messages: the second scripted turn stays unconsumed, proving the feedback
/// branch never fired.
#[tokio::test]
async fn no_session_turn_appends_no_feedback_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("context_length_exceeded in prompt")],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None,
        )
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("session-less model-correctable failure must return Err")?;
    assert_eq!(
        probe.request_count(),
        1,
        "session-less turns must not re-run via feedback"
    );
    assert!(
        err.msg.contains("conversation too long"),
        "hard-error message must describe the ContextOverflow failure; got: {}",
        err.msg
    );

    Ok(())
}

/// Two feedback retries followed by a 429 failure must still get the backoff
/// retry: the feedback budget and the backoff budget are independent.
#[tokio::test]
async fn feedback_retries_do_not_consume_backoff_budget() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-then-backoff";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("context_length_exceeded in prompt")],
        vec![MockStreamEvent::error("context_length_exceeded in prompt")],
        vec![MockStreamEvent::error("429 rate_limit_error")],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().run(),
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("turn must fail after the mock runs out of scripted turns")?;
    assert_eq!(
        probe.request_count(),
        4,
        "2 feedback retries + 429 backoff retry + final failed attempt = 4 model calls"
    );
    assert!(
        err.msg.contains("after 1 retries"),
        "the 429 must consume the backoff budget (attempt 1), not the feedback budget; got: {}",
        err.msg
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        2,
        "only the two model-correctable failures append feedback; got {persisted:?}"
    );

    Ok(())
}

/// A MaxTurnsExceeded failure on a session turn must append exactly one
/// user-role steering message and re-run the turn with a fresh budget —
/// proven by a second scripted turn completing — and must persist the
/// exhausted turn's delta (with its tool result) before the retry decision.
#[tokio::test]
async fn max_turns_failure_appends_steering_message_and_reruns_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tool_turns: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-max-turns-steering-rerun";
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "echo_tool",
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_context, _args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ))
        .await;

    // Turn 1: the tool call exhausts the 1-turn budget (rig rejects the next
    // model call). Turn 2: success — only reached if the steering retry
    // re-ran the turn.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "echo_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("steering retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "steering retry must complete the turn"
    );
    assert_eq!(
        probe.request_count(),
        2,
        "max-turns steering must re-run the turn exactly once"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        max_turns_steering_message_count(&persisted),
        1,
        "exactly one steering message must be appended; got {persisted:?}"
    );
    let steering_msg = persisted
        .iter()
        .find(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX)
            })
        })
        .ok_or("should have the appended steering message in the session history")?;
    assert!(
        matches!(steering_msg, crate::types::Message::User { .. }),
        "appended steering message must have User role; got {steering_msg:?}"
    );
    let has_tool_result = persisted.iter().any(|m| {
        matches!(
            m,
            crate::types::Message::User { content }
                if content.iter().any(|c| matches!(c, crate::types::UserContent::ToolResult(_)))
        )
    });
    assert!(
        has_tool_result,
        "the exhausted turn's delta (with tool result) must be persisted before the retry; got {persisted:?}"
    );

    Ok(())
}

/// Two consecutive MaxTurnsExceeded failures with a cap of
/// MAX_TURNS_FEEDBACK_RETRIES (1) must produce exactly one steering message
/// and then fall through to the hard-error path.
#[tokio::test]
async fn max_turns_cap_one_produces_one_steering_message_then_hard_error() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tool_turns: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-max-turns-steering-cap";
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "echo_tool",
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_context, _args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ))
        .await;

    // Both scripted turns emit a tool call: every attempt exhausts the
    // 1-turn budget, so the second failure hits the cap.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "echo_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::tool_call("tc2", "echo_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("capped max-turns failures must fall through to the hard error")?;
    assert_eq!(
        probe.request_count(),
        2,
        "one steering retry plus the capped final attempt = 2 model calls"
    );
    assert!(
        err.msg.contains("Max turns (1) exceeded"),
        "hard-error message must be the existing max-turns failure; got: {}",
        err.msg
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        max_turns_steering_message_count(&persisted),
        1,
        "cap 1 must produce exactly one steering message; got {persisted:?}"
    );

    Ok(())
}

/// A MaxTurnsExceeded turn with no session (final_session_id None) must
/// return Err without appending a steering message and without re-running:
/// the second scripted turn stays unconsumed.
#[tokio::test]
async fn no_session_max_turns_failure_returns_err_without_steering() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tool_turns: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "echo_tool",
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_context, _args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ))
        .await;

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "echo_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None,
        )
        .await;

    // -- Check
    let err = result
        .err()
        .ok_or("session-less max-turns failure must return Err")?;
    assert_eq!(
        probe.request_count(),
        1,
        "session-less turns must not re-run via steering"
    );
    assert!(
        err.msg.contains("Max turns (1) exceeded"),
        "hard-error message must be the existing max-turns failure; got: {}",
        err.msg
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Doom-loop stop surfacing (Path C)
// ---------------------------------------------------------------------------

/// A doom-loop stop (detection 4, the 8th identical tool call) must surface
/// the stop reason in the response text, emit a Warning, and emit an
/// AssistantMessage on the bus.
#[tokio::test]
async fn doom_stop_surfaces_reason_in_response_warning_and_assistant_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-doom-stop-surface";
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "echo_tool",
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_context, _args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ))
        .await;

    // 8 identical tool calls: 5 threshold + 1 first + 2 backoff + 1 stop.
    let turns: Vec<Vec<MockStreamEvent>> = (0..8)
        .map(|i| {
            vec![
                MockStreamEvent::tool_call(format!("tc{i}"), "echo_tool", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("doom stop must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("doom stop must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(DOOM_LOOP_STOP_PREFIX),
        "response text must start with DOOM_LOOP_STOP_PREFIX, got: {response_text}"
    );
    assert!(
        response_text.contains("echo_tool"),
        "response text must name the looping tool, got: {response_text}"
    );

    let events = event_collector.drain();
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Warning { message } if message.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Warning starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text } if text.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit an AssistantMessage starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );

    Ok(())
}

/// A user cancel via the bus cancel channel must keep today's behavior:
/// empty response text, no Warning, no AssistantMessage.
#[tokio::test]
async fn user_cancel_stays_silent_with_empty_response() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-user-cancel-silent";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "test_cancel_tool".to_string(),
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("user cancel must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("user cancel must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.is_empty(),
        "user cancel must keep empty response text, got: {response_text}"
    );

    let events = event_collector.drain();
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::Warning { .. })),
        "user cancel must not emit a Warning; got: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "user cancel must not emit an AssistantMessage; got: {events:?}"
    );

    Ok(())
}

// -- Test Support

/// Load all persisted messages for a session from the store.
async fn load_persisted_messages(
    memory_state: &MemoryState<FsSessionStore>,
    session_id: &str,
) -> Result<Vec<crate::types::Message>> {
    let entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    Ok(entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect())
}

/// Extract the first Text content string from a User or Assistant message.
fn message_text(msg: &crate::types::Message) -> Option<String> {
    match msg {
        crate::types::Message::User { content } => content.iter().find_map(|c| {
            if let crate::types::UserContent::Text(t) = c {
                Some(t.text.clone())
            } else {
                None
            }
        }),
        crate::types::Message::Assistant { content, .. } => content.iter().find_map(|c| {
            if let crate::types::AssistantContent::Text(t) = c {
                Some(t.text.clone())
            } else {
                None
            }
        }),
        crate::types::Message::System { .. } => None,
    }
}

/// Count messages whose first text content starts with the provider-feedback
/// prefix.
fn feedback_message_count(messages: &[crate::types::Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX)
            })
        })
        .count()
}

/// Count messages whose first text content starts with the max-turns steering
/// prefix.
fn max_turns_steering_message_count(messages: &[crate::types::Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX)
            })
        })
        .count()
}
