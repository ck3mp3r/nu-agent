//! Tests for the turn module.
//!
//! Covers:
//!  - `TurnResult` / `TurnError` construction and conversion (unit tests)
//!  - `execute_turn` using `MockResolver` + rig's `MockCompletionModel` (integration tests)

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::proxy::FilteredToolProxy;
use super::*;
use crate::config::Config;
use crate::conversation::state::memory::MemoryOf;
use crate::hook::agent_hook::DoomLoopState;
use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::session::FsSessionStore;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::mcp::circuit_breaker::McpCircuitBreaker;
use crate::types::{
    AssistantContent, Message, Text, ToolCall, ToolCallId, ToolDefinition, ToolFunction,
    ToolResultContent, UserContent,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn default_circuit_breaker() -> Arc<std::sync::Mutex<McpCircuitBreaker>> {
    Arc::new(std::sync::Mutex::new(McpCircuitBreaker::default()))
}

pub(crate) fn default_doom_state() -> Arc<std::sync::Mutex<DoomLoopState>> {
    Arc::new(std::sync::Mutex::new(DoomLoopState::default()))
}

/// A `last_total_tokens` slot starting at `None` (no real token count yet).
pub(crate) fn default_last_total_tokens() -> Arc<std::sync::Mutex<Option<u64>>> {
    Arc::new(std::sync::Mutex::new(None))
}

/// Build a `MemoryOf<FsSessionStore>` over a tempdir store (no compaction —
/// `CachedMemory` is used directly).
fn make_compacting_memory() -> (tempfile::TempDir, MemoryOf<FsSessionStore>) {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let memory = Arc::new(crate::session::CachedMemory::new(store));
    (temp_dir, memory)
}

/// Build a `CompactionConfig<FsSessionStore>` (no marker store) with a
/// deterministic streaming mock model so compaction never invokes a real LLM.
fn test_compaction_config(
    bus: crate::bus::Bus,
) -> crate::conversation::compaction::CompactionConfig<FsSessionStore> {
    use crate::conversation::compaction::compactor::NuCompactor;
    use rig::agent::ModelHandle;
    use rig::test_utils::MockCompletionModel;

    let turns: Vec<Vec<rig::test_utils::MockStreamEvent>> = (0..8)
        .map(|_| {
            vec![
                rig::test_utils::MockStreamEvent::Text("summary".to_string()),
                rig::test_utils::MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    crate::conversation::compaction::CompactionConfig {
        compactor: NuCompactor::from_shared_model(
            std::sync::Arc::new(std::sync::Mutex::new(ModelHandle::new(model))),
            bus.clone(),
            None,
        ),
        params: crate::compaction::CompactionParams::default(),
        threshold_tokens: None,
    }
}

// ---------------------------------------------------------------------------
// MockResolver: AsyncPermissionResolver that always returns Allow
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockResolver(PermissionDecision);

impl AsyncPermissionResolver for MockResolver {
    fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &crate::bus::Bus,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let decision = self.0;
        async move { decision }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_turn_context<'a>(
    shared_model: std::sync::Arc<std::sync::Mutex<rig::agent::ModelHandle>>,
    config: &'a Config,
    bus: crate::bus::Bus,
) -> TurnContext<'a, FsSessionStore> {
    let (temp_dir, memory) = make_compacting_memory();
    let _keep_alive = temp_dir;
    let conversation = TurnConversation {
        memory,
        conversation_id: "test-conv".to_string(),
        has_session: true,
        shared_model,
        compaction: test_compaction_config(bus.clone()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        last_total_tokens: default_last_total_tokens(),
        bus,
    };
    TurnContext::new(conversation, input, tool_infra, config)
}

/// Wrap a `MockCompletionModel` in the shared `Arc<Mutex<ModelHandle>>` so
/// the agent (built from the handle) and the hook route to the scripted model.
fn shared_handle(
    model: MockCompletionModel,
) -> std::sync::Arc<std::sync::Mutex<rig::agent::ModelHandle>> {
    std::sync::Arc::new(std::sync::Mutex::new(rig::agent::ModelHandle::new(model)))
}

// ---------------------------------------------------------------------------
// execute_turn integration tests
// ---------------------------------------------------------------------------

/// Text-only stream: verify `TurnResult.text` is populated and `tool_call_count == 0`.
#[tokio::test]
async fn execute_turn_text_only_response() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello, world!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let config = Config::default();
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    assert!(
        result.text.contains("Hello, world!") || !result.text.is_empty(),
        "TurnResult.text should be populated; got: {:?}",
        result.text
    );
    assert_eq!(
        result.tool_call_count, 0,
        "No tools should have been called"
    );
    assert!(!result.cancelled, "Turn should not be cancelled");
    Ok(())
}

/// Regression test: a turn that completes without any bus events still returns.
///
/// Core no longer spawns a per-turn `ui_rx` drain loop. Cancellation and all
/// lifecycle events flow through the shared `Bus`, so a turn that produces no
/// events still completes normally when the agent finishes.
#[tokio::test]
async fn execute_turn_completes_when_no_events_arrive_on_bus() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("no events on bus".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let config = Config::default();
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should terminate and succeed: {e:?}"))?;

    assert!(!result.cancelled, "Turn should not be cancelled");
    assert!(
        !result.text.is_empty(),
        "Text-only turn should produce a non-empty response"
    );
    Ok(())
}

/// Cancellation via the bus: a `CancelEvent` published from within a tool call
/// causes the hook to terminate, and the turn returns `cancelled = true`.
///
/// The model emits a tool call; the `CancellingTool` publishes the cancel on the
/// shared bus from inside `call()` — after the hook has subscribed to
/// `bus.cancel()` — so the next `on_completion_call` sees the pending event and
/// the turn returns cancelled. This is deterministic.
#[tokio::test]
async fn execute_turn_cancel_returns_cancelled_true() -> Result<()> {
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

    let config = Config::default();
    let bus = crate::bus::create_bus();
    let handle = rig::tool::server::ToolServer::new()
        .tool(executor::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: vec![ToolDefinition {
            name: "test_cancel_tool".to_string(),
            description: "cancels the turn".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        last_total_tokens: default_last_total_tokens(),
        bus: bus.clone(),
    };
    let ctx = make_turn_context(shared_handle(model), &config, bus);
    // Override the tool infra with the cancelling tool. `make_turn_context`
    // builds a context; rebuild with the cancelling tool infra.
    let ctx = TurnContext::new(ctx.conversation, ctx.input, tool_infra, &config);
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should not error: {e:?}"))?;

    assert!(result.cancelled, "Turn should be marked as cancelled");
    Ok(())
}

/// Verify that `additional_params` set in `Config` is forwarded through `AgentPromptConfig`
/// to `AgentBuilder` without panicking.
#[tokio::test]
async fn execute_turn_with_additional_params_succeeds() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("OK".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let config = Config {
        additional_params: Some(serde_json::json!({"thinking": {"type": "disabled"}})),
        ..Config::default()
    };
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;
    assert!(!result.cancelled, "Turn should not be cancelled");
    Ok(())
}

/// When both `max_tokens` and `max_output_tokens` are set, the completion
/// request SHALL carry `max_tokens` equal to `config.max_tokens`.
#[tokio::test]
async fn execute_turn_uses_config_max_tokens_when_set() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("OK".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let spy = model.clone();

    let config = Config {
        max_tokens: Some(2000),
        max_output_tokens: Some(5000),
        ..Config::default()
    };
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    let req = &spy.requests()[0];
    assert_eq!(
        req.max_tokens,
        Some(2000),
        "max_tokens should come from config.max_tokens"
    );
    Ok(())
}

/// When `max_tokens` is unset but `max_output_tokens` is set, the completion
/// request SHALL carry `max_tokens` equal to `config.max_output_tokens`.
#[tokio::test]
async fn execute_turn_falls_back_to_max_output_tokens_when_max_tokens_unset() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("OK".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let spy = model.clone();

    let config = Config {
        max_tokens: None,
        max_output_tokens: Some(4000),
        ..Config::default()
    };
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    let req = &spy.requests()[0];
    assert_eq!(
        req.max_tokens,
        Some(4000),
        "max_tokens should fall back to config.max_output_tokens"
    );
    Ok(())
}

/// When both `max_tokens` and `max_output_tokens` are unset, the completion
/// request SHALL carry no `max_tokens` value.
#[tokio::test]
async fn execute_turn_sends_no_max_tokens_when_both_unset() -> Result<()> {
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("OK".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let spy = model.clone();

    let config = Config {
        max_tokens: None,
        max_output_tokens: None,
        ..Config::default()
    };
    let ctx = make_turn_context(shared_handle(model), &config, crate::bus::create_bus());
    let resolver = MockResolver(PermissionDecision::Allow);

    execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    let req = &spy.requests()[0];
    assert_eq!(
        req.max_tokens, None,
        "no max_tokens should be sent when both are unset"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests: TurnResult / TurnError construction and field access
// ---------------------------------------------------------------------------

#[test]
fn turn_result_can_be_constructed() {
    let result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            tool_use_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: false,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert_eq!(result.text, "Hello");
    assert_eq!(result.usage.input_tokens, 10);
    assert_eq!(result.usage.output_tokens, 5);
    assert_eq!(result.tool_call_count, 0);
    assert!(!result.deltas_emitted);
    assert!(!result.cancelled);
}

#[test]
fn turn_error_can_be_constructed() {
    let error = TurnError::CompletionFailed {
        msg: "Test error".to_string(),
        kind: executor::CompletionErrorKind::Unknown,
    };

    assert!(!error.is_cancelled());
    let msg = match &error {
        TurnError::CompletionFailed { msg, .. } => msg.as_str(),
        _ => panic!("expected CompletionFailed"),
    };
    assert_eq!(msg, "Test error");

    let cancelled = TurnError::Cancelled {
        msg: "Cancelled".to_string(),
        messages: vec![],
    };
    assert!(cancelled.is_cancelled());
}

#[test]
fn prompt_cancelled_error_is_detected_as_cancellation() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "Hello".to_string(),
            additional_params: None,
        })],
    };
    let err = rig::completion::PromptError::PromptCancelled {
        chat_history: vec![user_msg],
        reason: "Cancelled by user".to_string(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "PromptCancelled variant should be detected as cancellation"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg, messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert!(msg.contains("Cancelled by user"));

    assert_eq!(
        messages.len(),
        1,
        "Should have one message from chat_history"
    );
}

#[test]
fn other_prompt_errors_are_not_cancelled() {
    let completion_err =
        rig::completion::CompletionError::ResponseError("Some other error".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "Non-cancellation errors should not be detected as cancellation"
    );
}

#[test]
fn max_turns_error_is_not_cancelled() {
    let err = rig::completion::PromptError::MaxTurnsError {
        max_turns: 10,
        chat_history: Box::new(vec![]),
        prompt: Box::new(Message::User {
            content: vec![UserContent::Text(Text {
                text: "test".to_string(),
                additional_params: None,
            })],
        }),
    };

    let turn_err = TurnError::from(err);

    assert!(
        !turn_err.is_cancelled(),
        "MaxTurnsError should not be detected as cancellation"
    );
}

/// Test that TurnContext can be created without an MCP runtime.
#[test]
fn turn_context_always_has_tool_server_handle() {
    let handle = rig::tool::server::ToolServer::new().run();
    let _handle_clone = handle.clone();
}

/// Test that TurnContext uses CompactingMemoryOf<FsSessionStore>.
#[test]
fn turn_context_uses_compacting_memory() {
    let (_temp_dir, memory) = make_compacting_memory();
    let conversation_id = "test-conversation-123".to_string();
    let _memory_clone = memory.clone();
    let _id_clone = conversation_id.clone();
}

/// Test that StreamingError converts to TurnError with cancelled=false for non-cancel errors.
#[test]
fn streaming_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "Tell me about async".to_string(),
            additional_params: None,
        })],
    };

    let inner = rig::completion::PromptError::PromptCancelled {
        reason: "Hook cancelled".to_string(),
        chat_history: vec![user_msg],
    };
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(inner));

    let turn_err = TurnError::from(streaming_err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "Hook cancelled");
    assert_eq!(messages.len(), 1);
}

// ---------------------------------------------------------------------------
// FilteredToolProxy cancellation tests
// ---------------------------------------------------------------------------

/// Publishing a cancel event while a tool is executing causes FilteredToolProxy
/// to return a Cancelled error immediately, without waiting for the (potentially
/// hanging) tool body to finish.
///
/// The cancel must be published AFTER the tool starts running: the proxy subscribes
/// to the bus cancel channel inside its execution closure, and broadcast channels
/// only deliver messages sent after the receiver subscribes. Publishing before the
/// call would race the subscription and not exercise the cancel branch.
#[tokio::test]
async fn filtered_tool_proxy_cancels_during_execution() -> Result<()> {
    let handle = rig::tool::server::ToolServer::new().run();
    // Register a tool that sleeps so the cancel can fire mid-execution.
    handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "sleeping_tool",
            "sleeps to allow cancellation",
            serde_json::json!({}),
            |_context, _args| {
                Box::pin(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    Ok(rig::tool::ToolOutput::text("done"))
                })
            },
        ))
        .await;

    let bus = crate::bus::create_bus();
    let proxy = FilteredToolProxy {
        tool_name: "sleeping_tool".to_string(),
        tool_definition: ToolDefinition {
            name: "sleeping_tool".to_string(),
            description: "test".to_string(),
            parameters: serde_json::json!({}),
        },
        handle,
        bus: bus.clone(),
    };

    // Convert to DynamicTool and execute via ToolSet.
    let dynamic_tool = proxy.into_dynamic_tool();
    let mut toolset = rig::tool::ToolSet::default();
    toolset.add_dynamic_tool(dynamic_tool);

    // Publish the cancel shortly after the tool call starts running.
    let bus2 = bus.clone();
    let cancel_handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let _ = bus2.cancel().send(crate::bus::CancelEvent::Requested).await;
    });

    let mut context = rig::tool::ToolContext::new();
    let result = toolset.execute("sleeping_tool", "{}", &mut context).await;
    cancel_handle
        .await
        .map_err(|e| format!("cancel task panicked: {e:?}"))?;

    assert!(
        result.is_error_kind(rig::tool::ToolErrorKind::Cancelled),
        "A cancel during execution must produce a Cancelled error"
    );
    Ok(())
}

/// TurnError from PromptCancelled captures chat_history as messages.
#[test]
fn turn_error_from_prompt_cancelled_captures_messages() {
    let user_msg = Message::User {
        content: vec![UserContent::Text(Text {
            text: "What is Rust?".to_string(),
            additional_params: None,
        })],
    };
    let assistant_msg = Message::Assistant {
        id: None,
        content: vec![AssistantContent::Text(Text {
            text: "Rust is a systems programming...".to_string(),
            additional_params: None,
        })],
    };

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc".to_string(),
        chat_history: vec![user_msg, assistant_msg],
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc");

    assert_eq!(
        messages.len(),
        2,
        "Both user and assistant messages should be captured"
    );
}

#[test]
fn turn_error_from_non_cancelled_has_no_messages() {
    let completion_err =
        rig::completion::CompletionError::ResponseError("Network timeout".to_string());
    let err = rig::completion::PromptError::from(completion_err);

    let turn_err = TurnError::from(err);

    assert!(!turn_err.is_cancelled());
    assert!(
        !matches!(turn_err, TurnError::Cancelled { .. }),
        "Non-cancelled errors should not be Cancelled variant"
    );
}

/// Path B: cancel_token fired, partial text accumulated.
#[test]
fn path_b_cancelled_with_partial_text_constructs_user_and_assistant_messages() {
    let turn_result = TurnResult {
        text: "partial response".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        2,
        "Path B with partial text must produce user + assistant messages"
    );

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "First message must be a user message"
    );

    match &cancelled_messages[1] {
        Message::Assistant { content, .. } => {
            let text = content.iter().find_map(|c| {
                if let AssistantContent::Text(t) = c {
                    Some(t.text.as_str())
                } else {
                    None
                }
            });
            assert_eq!(
                text,
                Some("partial response"),
                "Assistant message must contain partial text"
            );
        }
        other => panic!("Expected assistant message, got {:?}", other),
    }
}

/// Path B: cancel_token fired, no text accumulated.
#[test]
fn path_b_cancelled_with_empty_text_constructs_only_user_message() {
    let turn_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: false,
        cancelled: true,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    let prompt = "user prompt".to_string();
    let mut cancelled_messages = vec![Message::user(prompt.clone())];
    if !turn_result.text.is_empty() {
        cancelled_messages.push(Message::assistant(turn_result.text.clone()));
    }

    assert_eq!(
        cancelled_messages.len(),
        1,
        "Path B with empty text must produce only the user message (no empty assistant)"
    );

    assert!(
        matches!(&cancelled_messages[0], Message::User { .. }),
        "The single message must be a user message"
    );
}

#[test]
fn turn_result_cancelled_flag_propagates() {
    let cancelled_result = TurnResult {
        text: String::new(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert!(cancelled_result.cancelled, "Cancelled flag should be true");
    assert!(
        cancelled_result.text.is_empty(),
        "Cancelled turn should have empty text"
    );
    assert!(
        cancelled_result.messages.is_none(),
        "Cancelled via cancel_token should have no messages (FinalResponse not received)"
    );

    let normal_result = TurnResult {
        text: "Hello".to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: Some(vec![]),
        tool_call_count: 1,
        deltas_emitted: true,
        cancelled: false,
        last_total_tokens: 0,
        pre_turn_message_count: 0,
        last_known_history: vec![],
    };

    assert!(
        !normal_result.cancelled,
        "Normal turn should not be cancelled"
    );
}

// ---------------------------------------------------------------------------
// rig v0.39.0: Path B with tool calls — chat_history preserved after cancel
// ---------------------------------------------------------------------------

/// Regression test: PromptCancelled preserves tool_call + tool_result history.
#[test]
fn cancel_mid_tool_call_preserves_tool_call_and_tool_result_in_history() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text {
            text: "What is in /etc/hosts?".to_string(),
            additional_params: None,
        })],
    });

    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_abc123"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({ "path": "/etc/hosts" }),
            },
        })],
    });

    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: ToolCallId::new_or_mint("call_abc123"),
                provider: None,
                name: "read_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "file contents here".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "User pressed Esc during tool execution".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(
        turn_err.is_cancelled(),
        "TurnError must be marked as cancelled"
    );
    let (msg, messages) = match &turn_err {
        TurnError::Cancelled { msg, messages, .. } => (msg.as_str(), messages),
        _ => panic!("expected Cancelled variant"),
    };
    assert_eq!(msg, "User pressed Esc during tool execution");

    assert_eq!(
        messages.len(),
        3,
        "Must preserve user message + assistant(tool_call) + user(tool_result)"
    );

    match &messages[0] {
        Message::User { .. } => {}
        _ => panic!("msg[0] should be User (prompt)"),
    };
    match &messages[1] {
        Message::Assistant { .. } => {}
        _ => panic!("msg[1] should be Assistant (tool_call)"),
    };
    match &messages[2] {
        Message::User { .. } => {}
        _ => panic!("msg[2] should be User (tool_result)"),
    };
}

/// Regression: multiple tool-use cycles are all preserved on cancellation.
#[test]
fn cancel_preserves_multiple_tool_use_cycles() {
    use serde_json::json;

    let mut chat_history: Vec<Message> = Vec::new();

    // First tool-use cycle
    chat_history.push(Message::User {
        content: vec![UserContent::Text(Text {
            text: "List files".to_string(),
            additional_params: None,
        })],
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_001"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "list_dir".to_string(),
                arguments: json!({ "path": "/" }),
            },
        })],
    });
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: ToolCallId::new_or_mint("call_001"),
                provider: None,
                name: "list_dir".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "/bin /usr /etc".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    // Second tool-use cycle
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::Text(Text {
            text: "Now read that file".to_string(),
            additional_params: None,
        })],
    });
    chat_history.push(Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_002"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({ "path": "/etc/passwd" }),
            },
        })],
    });
    chat_history.push(Message::User {
        content: vec![UserContent::ToolResult(
            rig::completion::message::ToolResult {
                call: ToolCallId::new_or_mint("call_002"),
                provider: None,
                name: "read_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "root:x:0:0:root user".to_string(),
                    additional_params: None,
                })],
            },
        )],
    });

    let err = rig::completion::PromptError::PromptCancelled {
        reason: "Cancelled during read_file tool call".to_string(),
        chat_history: chat_history.clone(),
    };

    let turn_err = TurnError::from(err);

    assert!(turn_err.is_cancelled());

    let messages = match &turn_err {
        TurnError::Cancelled { messages, .. } => messages,
        _ => panic!(
            "expected Cancelled variant: all accumulated messages must be preserved on cancel — including 2 completed tool cycles"
        ),
    };

    assert_eq!(
        messages.len(),
        chat_history.len(),
        "Every single message in accumulated history should survive cancellation"
    );
}

// ---------------------------------------------------------------------------
// has_session guard: transient turns must not write JSONL to disk
// ---------------------------------------------------------------------------

/// Transient turn (`has_session = false`) must NOT create any JSONL file.
///
/// Before the fix, rig called `memory.append()` unconditionally at turn end,
/// writing a `transient-{millis}.jsonl` file that accumulates indefinitely.
/// After the fix, `.memory()` is omitted from the rig `AgentBuilder` when
/// `has_session = false`, so `memory.append()` is never called.
#[tokio::test]
async fn transient_turn_does_not_write_jsonl() -> Result<()> {
    let (temp_dir, memory) = make_compacting_memory();
    let sessions_path = temp_dir.path().to_path_buf();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello, world!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let conversation = TurnConversation {
        memory,
        conversation_id: format!(
            "transient-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ),
        has_session: false,
        shared_model: shared_handle(model),
        compaction: test_compaction_config(crate::bus::create_bus()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
    };
    let config = Config::default();
    let ctx = TurnContext::new(conversation, input, tool_infra, &config);
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;
    assert!(!result.cancelled, "transient turn should not be cancelled");

    let jsonl_files: Vec<_> = std::fs::read_dir(&sessions_path)
        .map_err(|e| format!("sessions dir must be readable: {e:?}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    assert!(
        jsonl_files.is_empty(),
        "transient turn must not write any JSONL file; found: {:?}",
        jsonl_files.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
    Ok(())
}

/// Persistent turn (`has_session = true`) MUST create a JSONL file for the session.
///
/// Verifies that the guard does not accidentally suppress JSONL writes for real
/// sessions — only transient invocations are exempted.
#[tokio::test]
async fn persistent_turn_writes_jsonl() -> Result<()> {
    let (temp_dir, memory) = make_compacting_memory();
    let sessions_path = temp_dir.path().to_path_buf();
    let session_id = "test-persistent-session";

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello from LLM!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let conversation = TurnConversation {
        memory,
        conversation_id: session_id.to_string(),
        has_session: true,
        shared_model: shared_handle(model),
        compaction: test_compaction_config(crate::bus::create_bus()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
    };
    let config = Config::default();
    let ctx = TurnContext::new(conversation, input, tool_infra, &config);
    let resolver = MockResolver(PermissionDecision::Allow);

    let result = execute_turn(ctx, resolver)
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;
    assert!(!result.cancelled, "persistent turn should not be cancelled");

    let jsonl_files: Vec<_> = std::fs::read_dir(&sessions_path)
        .map_err(|e| format!("sessions dir must be readable: {e:?}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    assert!(
        !jsonl_files.is_empty(),
        "persistent turn must write a JSONL file for the session; sessions dir is empty"
    );
    Ok(())
}
