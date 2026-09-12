//! TurnExecutor construction, compaction, and token-accounting tests.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::conversation::state::memory::MemoryState;
use crate::session::{FsSessionStore, StoreEntry};

#[test]
fn turn_executor_new_constructs_without_panic() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model_handle();

    let _executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );
    // Construction succeeded — no panic.
}

#[test]
fn turn_executor_exposes_memory_state() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model_handle();

    let executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    // Verify memory_state is accessible and last_total_tokens starts None
    assert!(executor.memory_state.last_total_tokens().is_none());
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);
    let shared_model = super::test_utils::shared_mock_model_handle();

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    assert!(executor.take_response_data().is_none());
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

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
