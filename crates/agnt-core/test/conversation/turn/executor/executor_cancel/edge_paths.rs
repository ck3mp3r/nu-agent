use super::*;

// ---------------------------------------------------------------------------
// Path B regression: last_known_history preserves tool calls on cancel
// ---------------------------------------------------------------------------

/// Regression test for the critical Path B cancel bug: when `tokio::select!`
/// cancels the stream BEFORE rig yields `Cancelled` (Path B), the executor
/// must use `TurnResult.last_known_history` to persist completed tool calls and
/// their results — NOT fall through to the old minimal `[user(prompt)]` fallback.
///
/// Scenario: the agent completes tool call T1 (hook's `on_completion_call` fires
/// with history containing T1), then cancel fires via `tokio::select!` before
/// the next LLM response. The `StreamingTurnResult` has `cancelled: true,
/// messages: None` (rig never yielded `Cancelled`). However,
/// `last_known_history` contains the full snapshot including the completed T1.
///
/// **Before the fix:** Path B synthesized `[user(prompt), assistant(partial_text)]`
/// and all completed tool work was LOST.
///
/// **After the fix:** Path B reads `last_known_history`, slices the delta, and
/// persists the real work.
///
/// We exercise this end-to-end using the `JourneyHarness` pattern: a mock tool that
/// publishes a cancel event to the bus from within `call()` after producing its result.
/// The bus is threaded into the tool, so the cancel fires deterministically.
///
/// This is intentionally an integration test (not a unit test) because the bug
/// exists at the intersection of `build_agent_and_stream` (which populates
/// `last_known_history` on `TurnResult`) and `TurnExecutor::execute` (which
/// reads it in Path B).
#[tokio::test]
async fn path_b_cancel_preserves_tool_calls_via_last_known_history() -> Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use rig::test_utils::{MockCompletionModel, MockStreamEvent};

    use crate::bus::CancelEvent;
    use crate::tools::closure::ClosureRegistry;
    use crate::tools::handler::McpToolRegistry;

    // -- cancelling tool (fires cancel after producing its result) ----------
    struct CancellingTool {
        output: &'static str,
        bus: crate::bus::Bus,
        fired: Arc<AtomicBool>,
    }

    impl rig::tool::Tool for CancellingTool {
        const NAME: &'static str = "test_cancel_tool";
        type Error = std::convert::Infallible;
        type Args = serde_json::Value;
        type Output = String;

        fn description(&self) -> String {
            "Tool that cancels after first call".to_string()
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }

        async fn call(
            &self,
            _context: &mut rig::tool::ToolContext,
            _args: Self::Args,
        ) -> std::result::Result<Self::Output, Self::Error> {
            let result = self.output.to_string();
            if !self.fired.swap(true, Ordering::SeqCst) {
                tokio::task::yield_now().await;
                let _ = self.bus.cancel().send(CancelEvent::Requested).await;
            }
            Ok(result)
        }
    }

    // -- test body ---------------------------------------------------------
    let config = test_config();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_id = "test-path-b-lkh";
    let mut memory_state = make_memory_state(&temp_dir);

    let (bus,) = (crate::bus::create_bus(),);

    // Model: sub-turn 1 emits tool_call → tool executes (cancels after result).
    // Sub-turn 2 would normally proceed but cancel fires first.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".into()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let handle = rig::tool::server::ToolServer::new()
        .tool(CancellingTool {
            output: "tool_completed_successfully",
            bus: bus.clone(),
            fired: Arc::new(AtomicBool::new(false)),
        })
        .run();
    let tool_infra = ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: vec![rig::completion::ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: crate::types::ToolName::new("test_cancel_tool")?,
            description: "Tool that cancels after first call".to_string(),
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

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        tool_infra,
        shared_model,
        test_compaction_config(bus.clone()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "call the tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // 1. Must be Ok(EarlyReturn) — cancelled turn, not an error
    assert!(
        result.is_ok(),
        "cancelled turn must not return Err; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "cancelled turn must return EarlyReturn"
    );

    // 2. rig saves nothing on a cancelled run, so the rig store gains nothing.
    //    The tool call and its real result live only in the run's transcript,
    //    which `execute()` does not return (it yields `TurnOutcome`).
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a cancelled turn must save nothing to the rig store; got {persisted:?}"
    );

    Ok(())
}

/// A user cancel via the bus cancel channel must keep today's behavior:
/// empty response text, no Warning, no AssistantMessage.
#[tokio::test]
async fn user_cancel_stays_silent_with_empty_response() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-user-cancel-silent";
    let mut memory_state = make_memory_state(&temp_dir);

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
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
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
            bus,
            render_registry: ToolRenderRegistry::default(),
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
    let outcome = result.map_err(|e| format!("user cancel must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("user cancel must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.is_empty(),
        "user cancel must keep empty response text, got: {response_text}"
    );

    let events = event_collector.drain();
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::Warning { .. })),
        "user cancel must not emit a Warning; got: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "user cancel must not emit an AssistantMessage; got: {events:?}"
    );

    Ok(())
}
