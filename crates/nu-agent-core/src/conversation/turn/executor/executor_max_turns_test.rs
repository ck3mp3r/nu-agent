//! Max-turns steering tests.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_compaction_config, test_config};
use super::*;
use crate::config::Config;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
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
