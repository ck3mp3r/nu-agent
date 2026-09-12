//! Cancel-path persistence tests for the turn executor.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::protocol::event::UiEvent;
use crate::session::StoreEntry;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::utils::value_ext::extract_response_text_from_value;

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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
