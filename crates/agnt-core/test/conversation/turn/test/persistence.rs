use super::*;

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
        dyn_model: shared_handle(model),
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
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
        render_registry: ToolRenderRegistry::default(),
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
        dyn_model: shared_handle(model),
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
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
        render_registry: ToolRenderRegistry::default(),
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
