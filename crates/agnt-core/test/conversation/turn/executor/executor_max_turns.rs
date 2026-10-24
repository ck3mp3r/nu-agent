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
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

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
    tool_server_handle.add_dynamic_tool(rig::tool::DynamicTool::new(
        crate::types::ToolName::new("echo_tool")?,
        "echoes a fixed result",
        serde_json::json!({"type": "object", "properties": {}}),
        |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
    ));

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
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("echo_tool")?,
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
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

    // The first attempt carries the bare prompt; the steering retry's prompt
    // carries the max-turns steering message (latest-wins carrier).
    let requests = probe.requests();
    assert_eq!(
        request_prompt(&requests[0]),
        "hello",
        "the first attempt must carry the bare prompt"
    );
    assert!(
        request_prompt(&requests[1])
            .starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX),
        "the steering retry's prompt must carry the max-turns steering message; got {:?}",
        request_prompt(&requests[1])
    );

    // The retry's model context carries the first attempt's committed work:
    // the persisted delta is loaded from the store and rides the retry request.
    let retry_history = &requests[1].chat_history;
    assert_eq!(
        retry_history.len(),
        3,
        "the retry request carries the first attempt's delta plus its own prompt; \
         got {retry_history:?}"
    );
    assert!(
        matches!(&*retry_history[2], crate::types::Message::User { content }
            if content.iter().any(|item| matches!(item,
                crate::types::UserContent::ToolResult(result)
                    if result.call == crate::types::CallId::from_wire("tc1")))),
        "the retry request carries the first attempt's tool result; got {retry_history:?}"
    );

    // The steering retry persisted the exhausted attempt's delta before
    // re-running, so the successful retry's rig save extends it: the first
    // attempt's prompt, tool call and tool result, then the retry's steered
    // prompt and reply.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        5,
        "the first attempt's delta plus the successful retry's exchange; got {persisted:?}"
    );
    assert_eq!(
        message_text(&persisted[0]).as_deref(),
        Some("hello"),
        "the steering retry persists the first attempt's prompt; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the first attempt's committed tool call is persisted; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the first attempt's committed tool result is persisted; got {persisted:?}"
    );
    assert!(
        message_text(&persisted[3])
            .is_some_and(|text| text
                .starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX)),
        "the retry's prompt carries the steering message; got {persisted:?}"
    );
    assert_eq!(
        message_text(&persisted[4]).as_deref(),
        Some("recovered"),
        "the retry's reply is persisted; got {persisted:?}"
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
    tool_server_handle.add_dynamic_tool(rig::tool::DynamicTool::new(
        crate::types::ToolName::new("echo_tool")?,
        "echoes a fixed result",
        serde_json::json!({"type": "object", "properties": {}}),
        |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
    ));

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
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("echo_tool")?,
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
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

    // The steering retry's prompt carries the max-turns steering message.
    let requests = probe.requests();
    assert!(
        request_prompt(&requests[1])
            .starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX),
        "the steering retry's prompt must carry the max-turns steering message; got {:?}",
        request_prompt(&requests[1])
    );

    // Both attempts persist their delta: the steering retry persisted the first
    // attempt's prompt, tool call and tool result, and the capped final attempt
    // persists its own steered prompt, tool call and tool result.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        6,
        "one delta per attempt: the steering retry's and the capped final attempt's; \
         got {persisted:?}"
    );
    assert!(
        message_text(&persisted[0]).is_some_and(|text| text.ends_with("hello")),
        "the first attempt's delta keeps the original prompt; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the first attempt's committed tool call is persisted; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the first attempt's committed tool result is persisted; got {persisted:?}"
    );
    assert!(
        message_text(&persisted[3])
            .is_some_and(|text| text
                .starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX)),
        "the capped attempt's delta starts from the steered prompt; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[4], crate::types::Message::Assistant(_)),
        "the capped attempt's committed tool call is persisted; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[5], crate::types::Message::User { .. }),
        "the capped attempt's committed tool result is persisted; got {persisted:?}"
    );

    Ok(())
}

/// A MaxTurnsExceeded turn with no session (final_session_id None) steers the
/// same way a session turn does: one steering retry, then the cap falls through
/// to the hard error. Nothing is persisted — there is no rig store.
#[tokio::test]
async fn no_session_max_turns_failure_steers_once_then_hard_errors() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_tool_turns: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle.add_dynamic_tool(rig::tool::DynamicTool::new(
        crate::types::ToolName::new("echo_tool")?,
        "echoes a fixed result",
        serde_json::json!({"type": "object", "properties": {}}),
        |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
    ));

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
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("echo_tool")?,
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
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

    // -- Check: the session-less turn steered and the retry succeeded
    let outcome =
        result.map_err(|e| format!("the steering retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the session-less steering retry must complete the turn"
    );
    assert_eq!(
        probe.request_count(),
        2,
        "a session-less turn steers once, then the retry runs"
    );

    // The steering retry's prompt carries the max-turns steering message.
    let requests = probe.requests();
    assert!(
        request_prompt(&requests[1])
            .starts_with(crate::conversation::turn::feedback::MAX_TURNS_FEEDBACK_PREFIX),
        "the session-less steering retry's prompt must carry the steering message; got {:?}",
        request_prompt(&requests[1])
    );

    Ok(())
}
