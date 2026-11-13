use super::*;

/// `on_outcome` stores the real API token count so the hook's compaction
/// threshold uses real usage, not the chars/4 estimate.
///
/// After a completed turn whose model reports `total_tokens > 0`, the count
/// lands in the turn's meter: the hook's next decision reads the wire's own
/// total from `meter.context_tokens()` (verified through the public executor
/// boundary).
#[tokio::test]
async fn on_outcome_stores_total_tokens() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-hook-total-tokens";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("response".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1234),
    ]]);
    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    let meter = default_meter();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            store_latch: Default::default(),
            pending_summary: Default::default(),
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            meter: meter.clone(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            bus: crate::bus::Bus::default(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        test_compaction_config(crate::bus::Bus::default()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
                task_id: None,
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(
        result.is_ok(),
        "turn must complete; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, ExecuteOutcome::Completed));
    assert_eq!(
        meter.context_tokens(),
        Some(1234),
        "hook must store the real total_tokens from on_outcome"
    );

    Ok(())
}
