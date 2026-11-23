use super::*;

/// A reply the output limit cut WITH a complete tool call is not an answerless
/// cut: the call is the answer, so the turn runs it and asks again. The next
/// completion's failure must then read its own kind — a 429 stays `RateLimit`,
/// and no output-budget steering, raise, or user message applies.
#[tokio::test]
async fn output_budget_finish_does_not_outlive_its_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-cut-finish-does-not-outlive-its-turn";
    let mut memory_state = make_memory_state(&temp_dir);

    let mut tool_infra = default_tool_infra(crate::bus::Bus::default());
    tool_infra
        .tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            crate::types::ToolName::new("echo_tool")?,
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ));
    tool_infra.visible_tool_definitions = vec![crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new("echo_tool")?,
        description: "echoes a fixed result".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }];

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "echo_tool", serde_json::json!({})),
            truncated_reply(),
        ],
        vec![MockStreamEvent::error("429 rate_limit_error")],
    ]);
    let shared_model = super::test_utils::shared_model_handle(model);
    let mut executor = make_executor(&config, &mut memory_state, shared_model, tool_infra);

    // -- Exec
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

    // -- Check
    let err = result.expect_err("the rate-limited turn must fail");
    assert!(
        err.error.msg.contains("rate limit"),
        "the 429 must keep its own kind; got: {}",
        err.error.msg
    );
    assert!(
        !err.error.msg.contains("output budget"),
        "the previous turn's cut must not reach this failure; got: {}",
        err.error.msg
    );
    Ok(())
}
