//! The flush's denied pairs: a cancelled batch's undispatched siblings.

use super::*;

use rig::tool::Tool;

use crate::hook::chain::CANCELLED_DISPATCH_MESSAGE;

/// A cancelled turn's batch holds three calls: the first runs and requests the
/// cancel, the second's dispatch consumes it and the hook's cancel arm publishes
/// its denied pair, and the third never dispatches. The executor's cancel arm
/// flushes the third as a denied pair carrying the cancelled error kind, in
/// commit order, before the Stopped notice.
///
/// The flush records nothing: the persisted log keeps rig's closure shape for
/// the sibling the stop skipped.
#[tokio::test]
async fn cancelled_turn_flushes_undispatched_siblings_as_denied_pairs() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-sibling-flush";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::tool_call("tc2", EchoTool::NAME, serde_json::json!({"x": 1})),
            MockStreamEvent::tool_call("tc3", EchoTool::NAME, serde_json::json!({"x": 2})),
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
            visible_tool_definitions: test_tool_definitions()?,
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

    let mut tool_events = Vec::new();
    while let Ok(event) = ui_event_rx.try_recv() {
        if let Some(summary) = tool_event_summary(&event) {
            tool_events.push(summary);
        }
    }
    let cancelled_kind = Some("cancelled".to_string());
    let cancelled_message = Some(CANCELLED_DISPATCH_MESSAGE.to_string());
    assert_eq!(
        tool_events,
        vec![
            ("test_cancel_tool".to_string(), None, None, None),
            ("test_cancel_tool".to_string(), Some(true), None, None),
            (EchoTool::NAME.to_string(), None, None, None),
            (
                EchoTool::NAME.to_string(),
                Some(false),
                cancelled_kind.clone(),
                cancelled_message.clone(),
            ),
            (EchoTool::NAME.to_string(), None, None, None),
            (
                EchoTool::NAME.to_string(),
                Some(false),
                cancelled_kind,
                cancelled_message,
            ),
        ],
        "the stop's undispatched sibling must publish its denied pair after the denied dispatched call"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "a cancelled turn persists its delta; got {persisted:?}"
    );
    let calls = match &persisted[1] {
        crate::types::Message::Assistant(assistant) => assistant.tool_calls().collect::<Vec<_>>(),
        other => {
            return Err(format!(
                "the committed batch must persist as an assistant message; got {other:?}"
            )
            .into());
        }
    };
    assert_eq!(
        calls.len(),
        3,
        "the committed batch holds all three calls; got {calls:?}"
    );

    let results = match &persisted[2] {
        crate::types::Message::User { content } => content,
        other => {
            return Err(format!(
                "the committed batch's results must persist as a user message; got {other:?}"
            )
            .into());
        }
    };
    let row = |call_id: &str| {
        results.iter().find_map(|item| match item {
            crate::types::UserContent::ToolResult(result) if result.call.to_string() == call_id => {
                Some(result)
            }
            _ => None,
        })
    };
    let ran = row("tc1").ok_or("the results message must answer tc1")?;
    assert_eq!(
        ran.content,
        vec![crate::types::ToolResultContent::text(
            "cancelling_tool_result"
        )],
        "the call that ran keeps its real result"
    );
    let flushed = row("tc3").ok_or("the results message must answer tc3")?;
    assert!(
        flushed.is_error,
        "the flushed sibling's row is rig's error placeholder"
    );
    let text = match flushed.content.as_slice() {
        [crate::types::ToolResultContent::Text(text)] => text.text.clone(),
        other => {
            return Err(format!("tc3's row must hold one text block; got {other:?}").into());
        }
    };
    assert_eq!(
        text, "No result provided",
        "the flush records nothing, so the sibling keeps rig's closure shape"
    );

    Ok(())
}

/// A turn that completes normally flushes nothing: the cancel arm is the only
/// place the flush runs.
#[tokio::test]
async fn completed_turn_emits_no_cancelled_dispatch_pair() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-sibling-flush-completed";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", EchoTool::NAME, serde_json::json!({"x": 1})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();

    let handle = rig::tool::server::ToolServer::new().tool(EchoTool).run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new(EchoTool::NAME)?,
                description: "Echoes a fixed result".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"x": {"type": "number"}}
                }),
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
    let outcome = result.map_err(|e| format!("a completed turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "a turn with no cancel completes"
    );

    let mut flushed = Vec::new();
    while let Ok(event) = ui_event_rx.try_recv() {
        if let UiEvent::ToolCompleted {
            name,
            error_kind,
            message,
            ..
        } = &event
            && (error_kind.as_deref() == Some("cancelled")
                || message.as_deref() == Some(CANCELLED_DISPATCH_MESSAGE))
        {
            flushed.push(name.clone());
        }
    }
    assert!(
        flushed.is_empty(),
        "a completed turn must not flush any denied pair; got {flushed:?}"
    );

    Ok(())
}
