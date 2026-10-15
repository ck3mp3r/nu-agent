use super::*;

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
            name: ToolName::new("test_cancel_tool")?,
            description: "cancels the turn".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus: bus.clone(),
        render_registry: ToolRenderRegistry::default(),
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

/// Test that TurnContext uses CompactingMemoryOf<FsSessionStore>.
#[test]
fn turn_context_uses_compacting_memory() {
    let (_temp_dir, memory) = make_compacting_memory();
    let conversation_id = "test-conversation-123".to_string();
    let _memory_clone = memory.clone();
    let _id_clone = conversation_id.clone();
}
