use super::*;

// ---------------------------------------------------------------------------
// Doom-loop stop surfacing (Path C)
// ---------------------------------------------------------------------------

/// A doom-loop stop (detection 4, the 8th identical tool call) must surface
/// the stop reason in the response text, emit a Warning, and emit an
/// AssistantMessage on the bus.
#[tokio::test]
async fn doom_stop_surfaces_reason_in_response_warning_and_assistant_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-doom-stop-surface";
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle.add_dynamic_tool(rig::tool::DynamicTool::new(
        crate::types::ToolName::new("echo_tool")?,
        "echoes a fixed result",
        serde_json::json!({"type": "object", "properties": {}}),
        |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
    ));

    // 8 identical tool calls: 5 threshold + 1 first + 2 backoff + 1 stop.
    let turns: Vec<Vec<MockStreamEvent>> = (0..8)
        .map(|i| {
            vec![
                MockStreamEvent::tool_call(format!("tc{i}"), "echo_tool", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

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
            bus,
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
    let outcome = result.map_err(|e| format!("doom stop must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("doom stop must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(DOOM_LOOP_STOP_PREFIX),
        "response text must start with DOOM_LOOP_STOP_PREFIX, got: {response_text}"
    );
    assert!(
        response_text.contains("echo_tool"),
        "response text must name the looping tool, got: {response_text}"
    );

    let events = event_collector.drain();
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Warning { message } if message.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Warning starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    // Fix 2: the stop reason rides `UiEvent::Stopped` (appended as a notice),
    // never `AssistantMessage` (which truncates the streamed block).
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Stopped { reason } if reason.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Stopped event starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text, .. } if text.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    Ok(())
}
