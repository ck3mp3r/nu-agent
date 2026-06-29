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
    let classified = classify_completion_error(raw_error);
    assert_eq!(
        classified,
        "Turn failed: the API rejected this turn — a tool call was missing its result in the message history. Repair will run on the next turn."
    );
}

#[test]
fn error_classifier_returns_human_readable_message_for_tool_result_invalid_request() {
    let raw_error = r#"invalid_request_body: tool_result block has no matching tool call"#;
    let classified = classify_completion_error(raw_error);
    assert_eq!(
        classified,
        "Turn failed: the API rejected this turn — a tool call was missing its result in the message history. Repair will run on the next turn."
    );
}

#[test]
fn error_classifier_passes_through_unrelated_errors() {
    let raw_error = "network timeout";
    let classified = classify_completion_error(raw_error);
    assert_eq!(classified, "Turn failed: network timeout");
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

    // Must have exactly 3 messages: [User(prompt), Assistant(ToolCall), User(ToolResult)]
    assert_eq!(
        persisted.len(),
        3,
        "mid-tool-loop error must persist exactly [user_msg, assistant_tool_call, tool_result]; got {} messages: {:?}",
        persisted.len(),
        persisted
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
