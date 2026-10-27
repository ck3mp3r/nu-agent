//! History-repair tests: a cancelled or failed turn persists a closed delta.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_compaction_config, test_config};
use super::*;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

// ---------------------------------------------------------------------------
// Cancelled and failed turns persist a closed delta
// ---------------------------------------------------------------------------

/// On Cancelled (Ok path, cancelled=true) the turn's delta is persisted: the
/// prompt, the committed tool call and its result.
///
/// We force a Cancelled scenario by triggering immediate cancellation.
/// The mock model emits a tool_call on its first (and only) turn; the UI
/// cancels immediately so rig returns Cancelled with a chat_history that
/// contains the Assistant(ToolCall) and its User(ToolResult).
#[tokio::test]
async fn cancelled_turn_with_a_tool_call_persists_the_closed_delta() -> Result<()> {
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
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            bus,
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
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

    // The cancelled turn's delta is persisted: the prompt, the committed tool
    // call and its result.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "a cancelled turn persists its delta; got: {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("call a tool"));
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the committed tool call is persisted; got: {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the committed tool result is persisted; got: {persisted:?}"
    );

    Ok(())
}

/// On UnknownToolCall (Err path) the turn's delta is persisted: the prompt, the
/// rejected tool call and the hook's error result.
#[tokio::test]
async fn unknown_tool_error_with_a_tool_call_persists_the_closed_delta() -> Result<()> {
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

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
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

    // The failed turn's delta is persisted: the prompt, the rejected tool call
    // and the hook's error result.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "a failed turn persists its delta; got: {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("use a tool"));
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the rejected tool call is persisted; got: {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the hook's error result is persisted; got: {persisted:?}"
    );

    Ok(())
}
