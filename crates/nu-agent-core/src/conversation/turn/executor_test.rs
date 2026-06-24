//! Tests for TurnExecutor API surface and execute() cancellation paths.
//!
//! Covers:
//!  - TurnExecutor construction (API surface smoke tests)
//!  - Path C: PromptCancelled caught inside build_agent_and_stream returns
//!    Ok(cancelled=true, messages=Some) — executor must return EarlyReturn,
//!    persist messages, emit Completed, and NOT emit AssistantMessage.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

    // 2. Conversation store must have been appended with the cancelled messages
    let persisted = memory_state
        .conversation_store()
        .load(session_id)
        .expect("conversation store load should succeed");
    assert!(
        !persisted.is_empty(),
        "cancelled turn messages must be persisted to conversation store (path C)"
    );

    // 2b. InMemoryConversationMemory must also have been updated
    let in_memory = rt
        .block_on(memory_state.memory().load(session_id))
        .expect("in-memory load should succeed");
    assert!(
        !in_memory.is_empty(),
        "cancelled turn messages must be persisted to InMemoryConversationMemory (path C)"
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
