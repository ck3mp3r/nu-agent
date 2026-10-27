//! The flush's previews: an undispatched `nu` call renders its preview.

use super::*;

use rig::tool::Tool;

/// A cancelled batch streams two `nu` calls the stop leaves undispatched. The
/// flush publishes each one's argument-sourced preview between its `Started`
/// and `Completed` sends, so the live transcript renders the same previewed
/// block rehydration produces. The dispatched `echo` call gets no preview: its
/// pair comes from the hook's cancel arm, which carries no display.
#[tokio::test]
async fn cancelled_turn_flushes_undispatched_nu_calls_with_preview_between_pair() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-sibling-flush-preview";
    let mut memory_state = make_memory_state(&temp_dir);

    let ls_arguments = serde_json::json!({"command": "ls"});
    let pwd_arguments = serde_json::json!({"command": "pwd"});
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::tool_call("tc2", EchoTool::NAME, serde_json::json!({"x": 1})),
            MockStreamEvent::tool_call("tc3", "nu", ls_arguments.clone()),
            MockStreamEvent::tool_call("tc4", "nu", pwd_arguments.clone()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // Subscribe BEFORE driving the turn: broadcast sends are not buffered for
    // later subscribers.
    let mut ui_event_rx = bus.ui_event().subscribe();

    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .tool(EchoTool)
        .run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: handle,
            visible_tool_definitions: test_tool_definitions_with_nu()?,
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
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "a cancelled turn returns EarlyReturn"
    );

    let mut events = Vec::new();
    while let Ok(event) = ui_event_rx.try_recv() {
        if let Some(projected) = flush_event(&event) {
            events.push(projected);
        }
    }
    let cancelled = Some("cancelled".to_string());
    let ls_json = serde_json::to_string(&ls_arguments)?;
    let pwd_json = serde_json::to_string(&pwd_arguments)?;
    assert_eq!(
        events,
        vec![
            FlushEvent::Started("test_cancel_tool".to_string()),
            FlushEvent::Completed {
                name: "test_cancel_tool".to_string(),
                success: true,
                error_kind: None,
            },
            FlushEvent::Started(EchoTool::NAME.to_string()),
            FlushEvent::Completed {
                name: EchoTool::NAME.to_string(),
                success: false,
                error_kind: cancelled.clone(),
            },
            FlushEvent::Started("nu".to_string()),
            FlushEvent::Preview {
                tool_key: format!("nu\n{ls_json}"),
                content: "ls".to_string(),
                language: "nu".to_string(),
            },
            FlushEvent::Completed {
                name: "nu".to_string(),
                success: false,
                error_kind: cancelled.clone(),
            },
            FlushEvent::Started("nu".to_string()),
            FlushEvent::Preview {
                tool_key: format!("nu\n{pwd_json}"),
                content: "pwd".to_string(),
                language: "nu".to_string(),
            },
            FlushEvent::Completed {
                name: "nu".to_string(),
                success: false,
                error_kind: cancelled,
            },
        ],
        "each flushed nu call must publish its preview between the Started and Completed sends"
    );

    Ok(())
}
