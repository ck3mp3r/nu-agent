//! Tests for TurnExecutor API surface and execute() cancellation paths.
//!
//! Covers:
//!  - TurnExecutor construction (API surface smoke tests)
//!  - Path C: PromptCancelled caught inside build_agent_and_stream returns
//!    Ok(cancelled=true, messages=Some) — executor must return EarlyReturn,
//!    persist messages, emit Completed, and NOT emit AssistantMessage.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rig::memory::ConversationMemory;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::*;
use crate::config::Config;
use crate::conversation::providers::CachedProviderClient;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::UiEvent;
use crate::session::ConversationStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

// ---------------------------------------------------------------------------
// MockUi for executor tests
// ---------------------------------------------------------------------------

struct MockUi {
    pub events: Vec<UiEvent>,
    cancel_flag: Arc<AtomicBool>,
}

impl MockUi {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Pre-set cancel so take_cancel_requested() fires on the very first drain
    /// loop iteration — causes cancel_token to be set before the spawned tokio
    /// task processes any stream event, which makes build_agent_and_stream return
    /// Ok(StreamingTurnResult { cancelled: true, messages: Some(chat_history) }).
    fn immediately_cancelled() -> Self {
        Self {
            events: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl ProgressUi for MockUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        self.cancel_flag.swap(false, Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// MockResolver — always allows
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockResolver;

impl AsyncPermissionResolver for MockResolver {
    fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = PermissionDecision::Allow;
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Helper to build a minimal Config for testing.
fn test_config() -> Config {
    Config {
        provider: "copilot".to_string(),
        provider_impl: None,
        model: "gpt-4o".to_string(),
        api_key: None,
        base_url: None,
        preamble: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tokens: None,
        max_tool_turns: None,
        temperature: None,
        read_timeout_secs: None,
    }
}

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
    assert!(
        !persisted.is_empty(),
        "cancelled turn messages must be persisted to JSONL store (path C)"
    );

    // 2b. JournalConversationMemory cache must also have been updated
    let in_memory = rt
        .block_on(memory_state.memory().load(session_id))
        .expect("in-memory load should succeed");
    assert!(
        !in_memory.is_empty(),
        "cancelled turn messages must be present in JournalConversationMemory cache (path C)"
    );

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
    let from_memory = rt
        .block_on(memory_state.memory().load(session_id))
        .expect("memory load should succeed");

    assert!(
        !from_store.is_empty(),
        "JSONL must have cancelled messages (via single memory.append())"
    );
    assert!(
        !from_memory.is_empty(),
        "in-memory cache must have cancelled messages (via single memory.append())"
    );

    // Both contain the same count — single write, consistent state
    assert_eq!(
        from_store.len(),
        from_memory.len(),
        "JSONL and cache must have identical message count — single write, no double-write"
    );
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

/// Network/CompletionError has no chat_history — executor falls back to persisting
/// a user+assistant pair so the session retains alternating turn structure.
/// A bare user message at JSONL end would violate alternation and cause the Copilot API
/// to reject all subsequent turns with 400 "No user query found".
#[test]
fn network_error_persists_user_and_assistant_error_placeholder() {
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

    // JSONL must contain exactly two messages: user prompt + assistant error placeholder
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("store load should succeed");
    assert_eq!(
        persisted.len(),
        2,
        "hard error must persist a user+assistant pair to maintain alternating turn structure; got {} messages",
        persisted.len()
    );
    // First persisted message must be the user prompt
    assert!(
        matches!(persisted[0], crate::types::Message::User { .. }),
        "persisted[0] must be a User message"
    );
    // Second persisted message must be an assistant error placeholder
    assert!(
        matches!(persisted[1], crate::types::Message::Assistant { .. }),
        "persisted[1] must be an Assistant message"
    );
    // The assistant placeholder must contain the failure marker
    let assistant_content = match &persisted[1] {
        crate::types::Message::Assistant { content, .. } => content
            .iter()
            .find_map(|c| {
                if let crate::types::AssistantContent::Text(t) = c {
                    Some(t.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => unreachable!(),
    };
    assert!(
        assistant_content.contains("Turn failed"),
        "assistant error placeholder must contain 'Turn failed'; got: {assistant_content:?}"
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
