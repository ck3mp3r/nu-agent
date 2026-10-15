use super::*;

/// An OutputBudget error must surface a user message naming `max_output_tokens` and
/// `--max-output-tokens`.
#[tokio::test]
async fn output_budget_error_surfaces_user_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-output-budget";
    let mut memory_state = make_memory_state(&temp_dir);

    // Four failing turns: attempts 1-3 each get one feedback retry (cap 3);
    // attempt 4 exceeds the cap, so the final break carries the OutputBudget
    // kind and the hard-error path surfaces the max_output_tokens message.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
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
    let err = result.expect_err("OutputBudget error must fail the turn");
    assert!(
        err.msg.contains("max_output_tokens"),
        "user message must mention max_output_tokens; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("--max-output-tokens"),
        "user message must mention --max-output-tokens; got: {}",
        err.msg
    );
    Ok(())
}
