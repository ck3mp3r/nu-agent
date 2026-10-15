use super::*;

/// A mock turn that streams reasoning-only content then a final response with
/// `FinishReason::Length` must reach rig's empty-truncation check and surface as
/// a Prompt-wrapped `CompletionError::ResponseError` (the defect shape). The
/// executor must classify it as OutputBudget and run the feedback-retry path.
#[tokio::test]
async fn prompt_wrapped_empty_output_length_reaches_feedback_retry() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-prompt-wrapped-length";
    let mut memory_state = make_memory_state(&temp_dir);

    // Reasoning-only + truncating Length final: rig's `turn_delivered_no_answer`
    // returns true (reasoning is not an answer) and `truncating_finish_reason`
    // returns Length, so the run errors with the ResponseError message.
    let length_final = rig::operation::Finish {
        usage: rig::completion::Usage::default(),
        reason: Some(rig::completion::FinishReason::Length),
        ..Default::default()
    };
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final.clone()),
        ],
        vec![
            MockStreamEvent::reasoning("thinking..."),
            MockStreamEvent::FinalResponse(length_final),
        ],
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
    let err = result.expect_err("empty-output Length must fail the turn");
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
