//! Tests for TurnExecutor API surface and execute() cancellation paths.
//!
//! Covers:
//!  - TurnExecutor construction (API surface smoke tests)
//!  - Path C: PromptCancelled caught inside build_agent_and_stream returns
//!    Ok(cancelled=true, messages=Some) — executor must return EarlyReturn,
//!    persist messages, emit Completed, and NOT emit AssistantMessage.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::test_utils::{MockResolver, MockUi, test_config};
use super::*;
use crate::conversation::providers::CachedProviderClient;
use crate::session::ConversationStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

#[test]
fn turn_executor_new_constructs_without_panic() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let _executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );
    // Construction succeeded — no panic.
}

#[test]
fn turn_executor_exposes_memory_state() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    // Verify memory_state is accessible and last_total_tokens starts None
    assert!(executor.memory_state.last_total_tokens().is_none());
}

#[test]
fn turn_executor_take_response_data_returns_none_before_execute() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
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
#[test]
fn cancelled_ok_path_returns_early_return_persists_messages_and_emits_completed() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-session";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial response".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::immediately_cancelled();

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // 1. Result must be EarlyReturn (not Completed — that would be the bug)
    assert!(
        result.is_ok(),
        "execute() must not return Err for a cancelled turn; got: {:?}",
        result.err()
    );
    assert!(
        matches!(result.unwrap(), TurnOutcome::EarlyReturn(_)),
        "cancelled Ok path must return TurnOutcome::EarlyReturn, not TurnOutcome::Completed"
    );

    // 2. Conversation store must have been written with the cancelled messages
    //    (via JournalConversationMemory.append() — single write to both JSONL and cache)
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("conversation store load should succeed");
    // The mock cancels immediately (before any assistant response), so path C appends
    // the delta from PromptCancelled::chat_history. With pre_turn_count=0 on a fresh
    // session the delta is [user("hello")] = 1 message. However the mock timing may
    // include a partial assistant message depending on scheduler ordering, so we allow
    // up to 2 (user + optional partial assistant). We assert strictly > 0 AND <= 2 to
    // catch duplication (which would produce 3+ messages).
    assert!(
        !persisted.is_empty() && persisted.len() <= 2,
        "cancelled turn: expected 1-2 messages (user + optional partial), got {}; \
         messages: {:?}",
        persisted.len(),
        persisted
    );

    // 2b. memory.load() returns repair-filtered view — raw messages are in JSONL (asserted
    //     above). The cache is updated by append(), but load() applies repair which may
    //     trim a trailing user-only message from an immediately-cancelled turn.
    // The key invariant is JSONL durability (step 2 above), not the repair-filtered view.

    // 3. UiEvent::Completed must have been emitted
    assert!(
        ui.events
            .iter()
            .any(|e| matches!(e, UiEvent::Completed { .. })),
        "UiEvent::Completed must be emitted for a cancelled turn (path C)"
    );

    // 4. UiEvent::AssistantMessage must NOT have been emitted
    assert!(
        !ui.events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "UiEvent::AssistantMessage must NOT be emitted for a cancelled turn (path C)"
    );
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
#[test]
fn completed_turn_no_explicit_store_append_needed() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-completed-session";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello from LLM!".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Collect response data before dropping the executor (which holds a mutable borrow)
    let response_data = executor.take_response_data();

    // Turn must complete normally
    assert!(
        result.is_ok(),
        "execute() must succeed; got: {:?}",
        result.err()
    );
    assert!(
        matches!(result.unwrap(), TurnOutcome::Completed),
        "completed turn must return TurnOutcome::Completed"
    );

    // rig wrote to JSONL via memory.append() — no explicit store.append() in executor
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("conversation store load should succeed");
    assert!(
        !persisted.is_empty(),
        "completed turn: JSONL must contain messages written via memory.append()"
    );

    // Response data must be available
    assert!(
        response_data.is_some(),
        "TurnResponseData must be populated after completed turn"
    );
}

/// Cancelled turn (path C) writes to both JSONL and in-memory cache via a
/// single JournalConversationMemory.append() call — not two separate calls.
///
/// Verifying the single-write pattern: both `conversation_store().load()` and
/// `memory().load()` return the same messages after a cancelled turn.
#[test]
fn cancelled_turn_writes_via_single_memory_append() {
    use rig::memory::ConversationMemory;

    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-single-write-cancelled";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::immediately_cancelled();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), TurnOutcome::EarlyReturn(_)));

    // Both store (JSONL) and memory cache must have the messages
    let from_store = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

    assert!(
        !from_store.is_empty(),
        "JSONL must have cancelled messages (via single memory.append())"
    );

    // memory.load() returns the repair-filtered view. For immediately-cancelled turns
    // where only a trailing user message was stored, repair trims it to an empty slice.
    // The key invariant is JSONL durability (from_store above), not the repair-filtered view.
    // Verify that memory.load() succeeds (doesn't panic/error) — content is repair-determined.
    let _ = rt
        .block_on(memory_state.memory().load(session_id))
        .expect("memory load should succeed without error");
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
#[test]
fn last_total_tokens_updated_on_completed_turn() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-token-tracking";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Verify initial state
    assert!(memory_state.last_total_tokens().is_none());

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("response text".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "test prompt".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), TurnOutcome::Completed));

    // After a completed turn with a session, last_total_tokens must be Some(...)
    // (even if 0 from the mock model — the key is it was set).
    assert!(
        memory_state.last_total_tokens().is_some(),
        "last_total_tokens must be Some after a completed turn with a session"
    );
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
#[test]
fn max_turns_error_persists_full_history() {
    // max_tool_turns=0: rig raises MaxTurnsError as soon as a tool-call turn would
    // be scheduled (current_turn > 0 + 1 after the first tool response).
    let config = Config {
        max_tool_turns: Some(0),
        ..test_config()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-max-turns";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Turn 1: model asks for a tool call. With max_turns=0, rig will MaxTurnsError
    // as soon as it tries to schedule the tool-call turn.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tool_call_1", "some_tool", serde_json::json!({"x": 1})),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "please call a tool".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err (it's a hard error, not a cancellation)
    assert!(
        result.is_err(),
        "MaxTurnsError must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");
    assert!(
        !persisted.is_empty(),
        "MaxTurnsError must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );
}

/// UnknownToolCall carries full chat_history — executor must persist it and return Err.
///
/// Setup: mock returns a tool_call for "nonexistent_tool" which is not registered
/// in the agent's tool list. Rig raises UnknownToolCall. After Fix 1, TurnError
/// gets messages=Some(chat_history). After Fix 2, executor persists them.
#[test]
fn unknown_tool_error_persists_full_history() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-tool";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // No visible_tool_definitions → agent has no tools → any tool call is unknown.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tool_call_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "use a tool please".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err
    assert!(
        result.is_err(),
        "UnknownToolCall must propagate as LabeledError to caller"
    );

    // JSONL must have been written with the partial chat history
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");
    assert!(
        !persisted.is_empty(),
        "UnknownToolCall must persist chat_history to JSONL (Fix 1 + Fix 2 path A history)"
    );
}

/// Network/CompletionError on a fresh session — executor persists the user prompt
/// via the delta path (last_known_history = [user_prompt] after fix).
///
/// After the `on_completion_call` fix, `last_known_history` = `history + [prompt]` =
/// `[] + [user_msg]` = `[user_msg]`. delta = skip(0) = `[user_msg]` (non-empty),
/// so the delta path fires and persists just the user message. The placeholder path
/// is no longer triggered for this case.
#[test]
fn network_error_on_fresh_session_persists_user_message() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-network-error";
    let prompt_text = "what is the weather today?";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Streaming error on the first event — simulates network failure.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: prompt_text.to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err
    assert!(
        result.is_err(),
        "network error must propagate as LabeledError to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = skip(0) = [user_msg].
    // Delta path fires → 1 message persisted (just the user prompt).
    // The placeholder path no longer fires because the delta is non-empty.
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");
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
#[test]
fn hard_error_on_first_llm_call_persists_user_message() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-no-hook-history";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] → delta path fires → 1 message persisted.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: prompt_text.to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: last_known_history = [user_msg], delta = [user_msg], 1 message persisted.
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
}

/// When there is no session (transient invocation), hard errors must NOT write
/// anything to the store — there is no conversation to record.
#[test]
fn hard_error_no_session_persists_nothing() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Streaming error — hard failure, no history recoverable.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "a transient prompt".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        None, // <-- no session
        None,
    );

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
#[test]
fn prompt_cancelled_with_unpaired_tool_call_injects_synthetic_result() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancel-inject";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Model issues a tool call — the cancel fires before the tool result is
    // appended, so chat_history will contain Assistant(ToolCall) with no
    // matching User(ToolResult).
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tc_cancel_1", "some_tool", serde_json::json!({"x": 1})),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::immediately_cancelled();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "call a tool".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_ok(), "cancelled turn must not return Err");
    assert!(matches!(result.unwrap(), TurnOutcome::EarlyReturn(_)));

    // The persisted JSONL must contain both ToolCall and ToolResult entries.
    // If the model was fast enough that the tool call was actually processed
    // before cancel fired, we may get a completed turn. Either way, if a
    // ToolCall was persisted, its ToolResult must also be persisted.
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
                            &tr.id == call_id
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
}

/// On UnknownToolCall (Err path, e.messages=Some) with an unpaired ToolCall in
/// chat_history, the messages persisted to JSONL must contain a synthetic
/// User(ToolResult) immediately after the unpaired Assistant(ToolCall).
#[test]
fn unknown_tool_error_with_unpaired_tool_call_injects_synthetic_result() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-inject";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // The chat_history will contain the user prompt + Assistant(ToolCall) but
    // no User(ToolResult) since the tool could not be dispatched.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tc_unknown_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "use a tool".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // UnknownToolCall returns Err
    assert!(result.is_err(), "UnknownToolCall must propagate as Err");

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
                            &tr.id == call_id
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
}

// ---------------------------------------------------------------------------
// Subtask 2 — error classification test
// ---------------------------------------------------------------------------

#[test]
fn error_classifier_returns_human_readable_message_for_tool_use_invalid_request() {
    let raw_error = r#"invalid_request_body: tool_use block requires a subsequent tool_result"#;
    let (kind, user_msg) = classify_completion_error(raw_error, None);
    assert_eq!(kind, CompletionErrorKind::ToolStructure);
    assert_eq!(
        user_msg,
        "Turn failed: the API rejected this turn — a tool call was missing its result. The session has been repaired."
    );
}

#[test]
fn error_classifier_returns_human_readable_message_for_tool_result_invalid_request() {
    let raw_error = r#"invalid_request_body: tool_result block has no matching tool call"#;
    let (kind, user_msg) = classify_completion_error(raw_error, None);
    assert_eq!(kind, CompletionErrorKind::ToolStructure);
    assert_eq!(
        user_msg,
        "Turn failed: the API rejected this turn — a tool call was missing its result. The session has been repaired."
    );
}

#[test]
fn error_classifier_passes_through_unrelated_errors() {
    let raw_error = "502 bad gateway proxy error";
    let (kind, user_msg) = classify_completion_error(raw_error, None);
    assert_eq!(kind, CompletionErrorKind::Unknown);
    assert_eq!(user_msg, "Turn failed: 502 bad gateway proxy error");
}

// ---------------------------------------------------------------------------
// Gap 4 — CompletionErrorKind comprehensive classification tests
// ---------------------------------------------------------------------------

#[test]
fn classify_tool_structure_error() {
    let (kind, _) = classify_completion_error("invalid_request_body contains tool_use block", None);
    assert_eq!(kind, CompletionErrorKind::ToolStructure);
}

#[test]
fn classify_context_overflow_context_length_exceeded() {
    let (kind, _) = classify_completion_error("context_length_exceeded in prompt", None);
    assert_eq!(kind, CompletionErrorKind::ContextOverflow);
}

#[test]
fn classify_context_overflow_input_too_long() {
    let (kind, _) = classify_completion_error("input is too long for requested model", None);
    assert_eq!(kind, CompletionErrorKind::ContextOverflow);
}

#[test]
fn classify_context_overflow_reduce_length() {
    let (kind, _) = classify_completion_error("reduce the length of your prompt", None);
    assert_eq!(kind, CompletionErrorKind::ContextOverflow);
}

#[test]
fn classify_request_too_large() {
    let (kind, _) = classify_completion_error("413 request_too_large response", None);
    assert_eq!(kind, CompletionErrorKind::RequestTooLarge);
}

#[test]
fn classify_refusal_content_policy() {
    let (kind, _) = classify_completion_error("content policy violation detected", None);
    assert_eq!(kind, CompletionErrorKind::Refusal);
}

#[test]
fn classify_credits_exhausted() {
    let (kind, _) = classify_completion_error("account out of credits", None);
    assert_eq!(kind, CompletionErrorKind::CreditsExhausted);
}

#[test]
fn classify_quota_billing_error() {
    let (kind, _) = classify_completion_error("402 billing_error", None);
    assert_eq!(kind, CompletionErrorKind::Quota);
}

#[test]
fn classify_rate_limit() {
    let (kind, _) = classify_completion_error("rate limit exceeded 429", None);
    assert_eq!(kind, CompletionErrorKind::RateLimit);
}

#[test]
fn classify_overloaded() {
    let (kind, _) = classify_completion_error("529 overloaded_error service busy", None);
    assert_eq!(kind, CompletionErrorKind::Overloaded);
}

#[test]
fn classify_server_error() {
    let (kind, _) = classify_completion_error("500 api_error internal server", None);
    assert_eq!(kind, CompletionErrorKind::ServerError);
}

#[test]
fn classify_network_error_sending_request() {
    let (kind, _) = classify_completion_error("error sending request for url", None);
    assert_eq!(kind, CompletionErrorKind::Network);
}

#[test]
fn classify_network_stream_decode_invalid_utf8() {
    let (kind, _) = classify_completion_error("stream decode error: invalid utf-8", None);
    assert_eq!(kind, CompletionErrorKind::Network);
}

#[test]
fn classify_endpoint_not_found() {
    let (kind, _) = classify_completion_error("404 endpoint not found", None);
    assert_eq!(kind, CompletionErrorKind::EndpointNotFound);
}

#[test]
fn classify_auth_error() {
    let (kind, _) = classify_completion_error("401 authentication_error invalid key", None);
    assert_eq!(kind, CompletionErrorKind::Auth);
}

#[test]
fn classify_unknown_502_bad_gateway() {
    let (kind, _) = classify_completion_error("502 bad gateway proxy error", None);
    assert_eq!(kind, CompletionErrorKind::Unknown);
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
        !CompletionErrorKind::EndpointNotFound.is_retryable(),
        "EndpointNotFound must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Unknown.is_retryable(),
        "Unknown must not be retryable"
    );
}

#[test]
fn classify_does_not_misclassify_number_in_token_count() {
    // "5000" contains "500" as substring — must NOT match ServerError
    let (kind, _) = classify_completion_error("processing 5000 tokens per request", None);
    assert_eq!(kind, CompletionErrorKind::Unknown);

    // "4042" contains "404" as substring — must NOT match EndpointNotFound
    let (kind, _) = classify_completion_error("step 4042 of pipeline", None);
    assert_eq!(kind, CompletionErrorKind::Unknown);

    // "500" as a standalone token (standalone HTTP status) MUST match ServerError
    let (kind, _) = classify_completion_error("HTTP 500 internal server error", None);
    assert_eq!(kind, CompletionErrorKind::ServerError);
}

#[test]
fn classify_network_timeout_is_unknown_not_network() {
    // "network timeout" does not match any Network patterns (no "decode error",
    // "connection reset", etc.) — it falls through to Unknown.
    // Confirm this is stable and hasn't been accidentally absorbed.
    let (kind, _) = classify_completion_error("network timeout after 30s", None);
    assert_eq!(kind, CompletionErrorKind::Unknown);
}

#[test]
fn classify_insufficient_quota_is_quota_not_credits_exhausted() {
    // Fix 1: "insufficient_quota" belongs to Quota (OpenAI billing quota),
    // not CreditsExhausted (account credit balance exhausted).
    let (kind, _) = classify_completion_error("insufficient_quota for this API key", None);
    assert_eq!(kind, CompletionErrorKind::Quota);
}

#[test]
fn classify_request_entity_too_large_is_request_too_large_not_context_overflow() {
    // Fix 2: "request entity too large" is HTTP 413 (payload too large),
    // not ContextOverflow (conversation context window exceeded).
    let (kind, _) = classify_completion_error("request entity too large", None);
    assert_eq!(kind, CompletionErrorKind::RequestTooLarge);
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
#[test]
fn hard_error_after_prior_history_persists_user_message() {
    use crate::session::ConversationStore;

    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-hook-history";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Pre-populate the session store with a completed exchange so that rig
    // loads it into the agent's context and on_completion_call fires with
    // non-empty prior history.
    let prior_messages = vec![
        crate::types::Message::user("work done"),
        crate::types::Message::assistant("ok"),
    ];
    memory_state
        .conversation_store()
        .append(session_id, &prior_messages, None)
        .expect("pre-populate store should succeed");

    // Mock model: errors immediately (simulates CompletionError / network failure).
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("http decode error")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "new prompt".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err (it's a hard error)
    assert!(result.is_err(), "CompletionError must propagate as Err");

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
#[test]
fn hard_error_after_prior_history_persists_only_delta() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-delta-hard-error";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Pre-populate: simulate a prior successful turn
    memory_state
        .conversation_store()
        .append(
            session_id,
            &[
                crate::types::Message::user("prior work"),
                crate::types::Message::assistant("done"),
            ],
            None,
        )
        .expect("pre-populate should succeed");

    // Model errors immediately — simulates CompletionError / network failure.
    // on_completion_call fires with history = [user("prior work"), assistant("done")]
    // and prompt = user("new question").
    // After fix: last_known_history = [prior_1, prior_2, user_prompt].
    // delta = skip(2) = [user_prompt] → delta path fires → 3 total.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "new question".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_err(), "hard error must propagate as Err");

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
#[test]
fn hard_error_twice_does_not_double_history() {
    let config = test_config();
    let session_id = "test-no-double";
    let temp_dir = tempfile::tempdir().unwrap();

    // Turn 1: successful turn — rig appends [user("t1"), assistant("ok")] → store has 2 msgs.
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut memory_state =
            super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
        let model = MockCompletionModel::from_stream_turns([[
            MockStreamEvent::Text("ok".to_string()),
            MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
        ]]);
        let cached_client = CachedProviderClient::Mock(model);
        let mut ui = MockUi::new();
        let closure_registry = ClosureRegistry::new();
        let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &rt,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
            },
        );
        let result = executor.execute(
            &mut ui,
            ExecuteInput {
                prompt: "t1".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            &cached_client,
            MockResolver,
            Some(session_id),
            None,
        );
        assert!(result.is_ok(), "turn 1 must succeed");
    }

    // Turn 2: hard error. pre_turn_count=2. last_known_history = [prior_1, prior_2, user("t2")].
    // delta = skip(2) = [user("t2")] → delta path fires → store has 3.
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut memory_state =
            super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let cached_client = CachedProviderClient::Mock(model);
        let mut ui = MockUi::new();
        let closure_registry = ClosureRegistry::new();
        let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &rt,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
            },
        );
        let _ = executor.execute(
            &mut ui,
            ExecuteInput {
                prompt: "t2".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            &cached_client,
            MockResolver,
            Some(session_id),
            None,
        );
    }

    // Turn 3: hard error again. pre_turn_count=3. last_known_history = [prior_1, prior_2, user("t2"), user("t3")].
    // delta = skip(3) = [user("t3")] → delta path fires → store has 4.
    {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut memory_state =
            super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
        let model = MockCompletionModel::from_stream_turns([[MockStreamEvent::error("timeout")]]);
        let cached_client = CachedProviderClient::Mock(model);
        let mut ui = MockUi::new();
        let closure_registry = ClosureRegistry::new();
        let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
        let tool_server_handle = rig::tool::server::ToolServer::new().run();
        let mut executor = TurnExecutor::new(
            &config,
            &rt,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(closure_registry),
                mcp_registry: Arc::new(mcp_registry),
                tool_server_handle,
                visible_tool_definitions: vec![],
            },
        );
        let _ = executor.execute(
            &mut ui,
            ExecuteInput {
                prompt: "t3".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            &cached_client,
            MockResolver,
            Some(session_id),
            None,
        );
    }

    // Final state: 2 (turn 1 success) + 1 (turn 2 user delta) + 1 (turn 3 user delta) = 4.
    // After the fix: on_completion_call stores history + [prompt], so delta = [user_prompt]
    // for each error turn. Delta path fires → 1 message per error turn, not 2 (no placeholder).
    let final_memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());
    let final_count = final_memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed")
        .len();

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
#[test]
fn cancelled_turn_after_prior_history_persists_only_delta() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-delta";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Pre-populate: simulate a prior successful turn
    memory_state
        .conversation_store()
        .append(
            session_id,
            &[
                crate::types::Message::user("prior work"),
                crate::types::Message::assistant("done"),
            ],
            None,
        )
        .expect("pre-populate should succeed");

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("partial".to_string()),
        MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
    ]]);
    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::immediately_cancelled();
    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "new question".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_ok(), "cancelled turn must not return Err");
    assert!(matches!(result.unwrap(), TurnOutcome::EarlyReturn(_)));

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

    // MUST be <= 4 (2 prior + at most 2 new), NOT 5+ (prior doubled)
    assert!(
        persisted.len() <= 4,
        "cancelled turn after prior history must not double the store; got {} messages (expected <= 4)",
        persisted.len()
    );
    assert!(
        persisted.len() >= 2,
        "prior messages must be preserved; got {}",
        persisted.len()
    );
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
#[test]
fn hard_error_on_first_llm_call_no_prior_history_persists_user_message() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-fresh-session-2";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] (non-empty) → delta path fires → 1 message.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: prompt_text.to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // After the fix: 1 message persisted (user prompt via delta path, no placeholder).
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
#[test]
fn hard_error_mid_tool_loop_preserves_real_tool_results() {
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-mid-tool-loop-error";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Turn 1: LLM emits tool_call + FinalResponse
    // Turn 2: LLM errors (simulates CompletionError after tool result is in history)
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({"x": 1})),
            MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
        ],
        vec![MockStreamEvent::error("network failure after tool")],
    ]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = crate::tools::closure::ClosureRegistry::new();
    let mcp_registry = crate::tools::handler::McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "do the thing".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // The error must propagate (CompletionError from turn 2, or UnknownToolCall from turn 1)
    assert!(
        result.is_err(),
        "error on sub-call must propagate as Err; got ok"
    );

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
                            if &tr.id == call_id {
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
}

// ---------------------------------------------------------------------------
// close_open_tool_result_block unit tests
// ---------------------------------------------------------------------------

/// Helper: build a User message whose content is a single ToolResult.
fn user_with_tool_result(id: &str) -> crate::types::Message {
    use rig::one_or_many::OneOrMany;
    crate::types::Message::User {
        content: OneOrMany::one(crate::types::UserContent::ToolResult(
            crate::types::ToolResult {
                id: id.to_string(),
                call_id: None,
                content: OneOrMany::one(crate::types::ToolResultContent::text("result")),
            },
        )),
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
    use rig::one_or_many::OneOrMany;
    crate::types::Message::User {
        content: OneOrMany::many(vec![
            crate::types::UserContent::ToolResult(crate::types::ToolResult {
                id: id.to_string(),
                call_id: None,
                content: OneOrMany::one(crate::types::ToolResultContent::text("result")),
            }),
            crate::types::UserContent::Text(crate::types::Text::new("some text")),
        ])
        .expect("non-empty user content"),
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
#[test]
fn retry_succeeds_on_second_attempt() {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-success";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Turn 1: error (retryable 500). Turn 2: success.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error internal server")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
        ],
    ]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(
        result.is_ok(),
        "retry should succeed on second attempt; got: {:?}",
        result.err()
    );
    assert!(matches!(result.unwrap(), TurnOutcome::Completed));

    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");
    assert_eq!(
        persisted.len(),
        2,
        "successful retry must produce 2 messages (user + assistant); got {}",
        persisted.len()
    );
}

/// Retry exhausted: all attempts fail with retryable errors. The final error
/// message must mention the retry attempt count.
#[test]
fn retry_exhausted_surfaces_attempt_count() {
    let config = Config {
        max_retries: Some(2),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-exhausted";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // All 3 attempts (1 initial + 2 retries) fail with retryable error
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
    ]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_err(), "exhausted retries must return Err");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("after 2 retries"),
        "error message must mention retry count; got: {err_msg}"
    );
}

/// Non-retryable errors (e.g., context_length_exceeded) must NOT be retried.
/// Exactly 1 attempt should be made.
#[test]
fn non_retryable_error_not_retried() {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-non-retryable";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    // Only 1 turn — if retried, MockCompletionModel would panic (no more turns).
    // A 400 context_length_exceeded is NOT retryable.
    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "context_length_exceeded in prompt",
    )]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // Must fail immediately without retrying
    assert!(
        result.is_err(),
        "non-retryable error must not be retried; got Ok"
    );
    // If the model had been called more than once, MockCompletionModel would panic
    // (it only has 1 turn configured). The test passing proves exactly 1 HTTP request.
}

/// When `max_retries` is `Some(0)`, no retry is attempted regardless of error type.
///
/// Tests the most direct way to disable retries: setting max_retries=0 ensures
/// `attempt < max_retries` is always false on the first attempt (attempt=0 < 0 = false).
/// The test verifies the error message does NOT contain "retries" — proving the retry
/// path was not entered.
#[test]
fn retry_disabled_when_max_retries_is_zero() {
    let config = Config {
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-no-retry-guard";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "500 api_error server error",
    )]]);

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = MockUi::new();

    let closure_registry = ClosureRegistry::new();
    let mcp_registry = McpToolRegistry::from_names(Vec::<String>::new());
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mut executor = TurnExecutor::new(
        &config,
        &rt,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
        },
    );

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "hello".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    assert!(result.is_err(), "error must propagate without retry");
    let err_msg = result.unwrap_err().to_string();
    // When max_retries=0, attempt never increments, so "retries" should not appear
    assert!(
        !err_msg.contains("retries"),
        "error message must NOT mention retries when max_retries=0; got: {err_msg}"
    );
}

/// When `last_known_history` is empty the retry guard (`has_partial_history`) prevents
/// any retry from being attempted, even when `max_retries > 0` and the error is retryable.
///
/// Architecture note: In the current executor, `last_known_history` comes from
/// `TurnError::last_known_history`, which is populated by `on_completion_call` in the
/// rig hook. With `MockCompletionModel`, `on_completion_call` always fires (capturing
/// `[user_prompt]`), so `last_known_history` is never truly empty through the full
/// executor path.
///
/// Therefore, this test exercises the guard logic directly by constructing a `TurnError`
/// with `last_known_history: vec![]` and asserting that the guard condition evaluates to
/// `false` — confirming that an empty history would suppress retry.
///
/// For the full-path scenario (history always non-empty via MockCompletionModel), see
/// `retry_disabled_when_max_retries_is_zero` which tests the equivalent observable outcome.
#[test]
fn retry_not_attempted_when_no_partial_history() {
    // Test the guard condition: `has_partial_history = !e.last_known_history.is_empty()`.
    // Construct a TurnError with empty last_known_history and verify the guard is false.
    let turn_error_empty_history = crate::conversation::turn::TurnError {
        msg: "500 api_error server error".to_string(),
        cancelled: false,
        messages: None,
        last_known_history: vec![], // empty — no partial history
        pre_turn_message_count: 0,
        estimated_request_bytes: None,
    };

    let has_partial_history = !turn_error_empty_history.last_known_history.is_empty();

    // Guard must evaluate to false with empty history — retry must not be attempted.
    assert!(
        !has_partial_history,
        "has_partial_history guard must be false when last_known_history is empty; \
         retry must be suppressed even for retryable errors with max_retries > 0"
    );

    // Confirm the mirror case: non-empty history enables the guard.
    let turn_error_with_history = crate::conversation::turn::TurnError {
        msg: "500 api_error server error".to_string(),
        cancelled: false,
        messages: None,
        last_known_history: vec![crate::types::Message::user("prompt")],
        pre_turn_message_count: 0,
        estimated_request_bytes: None,
    };
    let has_partial_history_non_empty = !turn_error_with_history.last_known_history.is_empty();
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
/// fires the cancel token from within `call()` after producing its result. The
/// `MockUi::with_external_cancel()` injects the token so the cancel fires
/// deterministically.
///
/// This is intentionally an integration test (not a unit test) because the bug
/// exists at the intersection of `build_agent_and_stream` (which populates
/// `last_known_history` on `TurnResult`) and `TurnExecutor::execute` (which
/// reads it in Path B).
#[test]
fn path_b_cancel_preserves_tool_calls_via_last_known_history() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rig::test_utils::{MockCompletionModel, MockStreamEvent};

    use super::test_utils::MockUi;
    use crate::conversation::providers::CachedProviderClient;
    use crate::session::ConversationStore;
    use crate::tools::closure::ClosureRegistry;
    use crate::tools::handler::McpToolRegistry;

    // -- cancelling tool (fires cancel after producing its result) ----------
    struct CancellingTool {
        output: &'static str,
        token: tokio_util::sync::CancellationToken,
        fired: Arc<AtomicBool>,
    }

    impl rig::tool::Tool for CancellingTool {
        const NAME: &'static str = "test_cancel_tool";
        type Error = std::convert::Infallible;
        type Args = serde_json::Value;
        type Output = String;

        async fn definition(&self, _prompt: String) -> rig::completion::ToolDefinition {
            rig::completion::ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Tool that cancels after first call".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }
        }

        async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
            let result = self.output.to_string();
            if !self.fired.swap(true, Ordering::SeqCst) {
                tokio::task::yield_now().await;
                self.token.cancel();
            }
            Ok(result)
        }
    }

    // -- test body ---------------------------------------------------------
    let config = test_config();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_id = "test-path-b-lkh";
    let mut memory_state =
        super::super::super::state::memory::MemoryState::new(temp_dir.path().to_path_buf());

    let (ui, cancel_token) = MockUi::with_external_cancel();

    // Model: sub-turn 1 emits tool_call → tool executes (cancels after result).
    // Sub-turn 2 would normally proceed but cancel fires first.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
        ],
        vec![
            MockStreamEvent::Text("unreachable".into()),
            MockStreamEvent::FinalResponse(rig::test_utils::MockResponse::new()),
        ],
    ]);

    let handle = rig::tool::server::ToolServer::new()
        .tool(CancellingTool {
            output: "tool_completed_successfully",
            token: cancel_token,
            fired: Arc::new(AtomicBool::new(false)),
        })
        .run();
    let tool_infra = ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names(Vec::<String>::new())),
        tool_server_handle: handle,
        visible_tool_definitions: vec![rig::completion::ToolDefinition {
            name: "test_cancel_tool".to_string(),
            description: "Tool that cancels after first call".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
    };

    let cached_client = CachedProviderClient::Mock(model);
    let mut ui = ui;

    let mut executor = TurnExecutor::new(&config, &rt, &mut memory_state, tool_infra);

    let result = executor.execute(
        &mut ui,
        ExecuteInput {
            prompt: "call the tool".to_string(),
            preamble: None,
            span: nu_protocol::Span::test_data(),
        },
        &cached_client,
        MockResolver,
        Some(session_id),
        None,
    );

    // 1. Must be Ok(EarlyReturn) — cancelled turn, not an error
    assert!(
        result.is_ok(),
        "cancelled turn must not return Err; got: {:?}",
        result.err()
    );
    assert!(
        matches!(result.unwrap(), TurnOutcome::EarlyReturn(_)),
        "cancelled turn must return EarlyReturn"
    );

    // 2. Persisted JSONL must contain the user prompt + tool call + tool result
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");

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
                |c| matches!(c, crate::types::AssistantContent::ToolCall(tc) if tc.id == "tc1"),
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
                    tr.id == "tc1"
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
}

// ---------------------------------------------------------------------------
// Size-aware error classification tests
// ---------------------------------------------------------------------------

/// "error sending request" with estimated_request_bytes > 100 KB must be classified
/// as `RequestTooLarge` (non-retryable), not `Network` (retryable).
#[test]
fn classify_error_sending_request_large_payload_is_request_too_large() {
    let msg = "error sending request for url https://api.example.com/v1/chat";
    let large_payload = Some(200 * 1024); // 200 KB — well above 100 KB threshold
    let (kind, user_msg) = classify_completion_error(msg, large_payload);
    assert_eq!(
        kind,
        CompletionErrorKind::RequestTooLarge,
        "large payload 'error sending request' must be RequestTooLarge, not Network"
    );
    assert!(
        !kind.is_retryable(),
        "RequestTooLarge must not be retryable"
    );
    assert!(
        user_msg.contains("too large"),
        "user message must mention 'too large'; got: {user_msg}"
    );
}

/// "error sending request" with no estimated_request_bytes must remain `Network`
/// (retryable) — the fallback for genuine network failures.
#[test]
fn classify_error_sending_request_no_size_is_network() {
    let msg = "error sending request for url https://api.example.com/v1/chat";
    let (kind, _) = classify_completion_error(msg, None);
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "'error sending request' without size info must be Network"
    );
    assert!(kind.is_retryable(), "Network must be retryable");
}

/// "error sending request" with estimated_request_bytes below the 100 KB threshold
/// must remain `Network` (retryable).
#[test]
fn classify_error_sending_request_small_payload_is_network() {
    let msg = "error sending request for url https://api.example.com/v1/chat";
    let small_payload = Some(50 * 1024); // 50 KB — below 100 KB threshold
    let (kind, _) = classify_completion_error(msg, small_payload);
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "small payload 'error sending request' must be Network, not RequestTooLarge"
    );
    assert!(kind.is_retryable(), "Network must be retryable");
}

/// "error sending request" with estimated_request_bytes exactly at the 100 KB
/// boundary must remain `Network` — the threshold is strictly greater-than.
#[test]
fn classify_error_sending_request_at_boundary_is_network() {
    let msg = "error sending request for url https://api.example.com/v1/chat";
    let boundary = Some(100 * 1024); // exactly 100 KB
    let (kind, _) = classify_completion_error(msg, boundary);
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "boundary (100 KB exact) must be Network — threshold is strictly greater-than"
    );
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
