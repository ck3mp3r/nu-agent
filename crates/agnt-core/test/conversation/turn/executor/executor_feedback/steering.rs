use super::*;

/// A model-correctable provider failure classified by HTTP status (413 →
/// RequestTooLarge via `classify_by_status`) must steer the retry prompt with
/// exactly one feedback message and re-run the turn — proven by a second
/// scripted turn succeeding.
#[tokio::test]
async fn model_correctable_failure_steers_and_reruns_turn() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-retry-rerun";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: 413 classified by status to RequestTooLarge (model-correctable).
    // Turn 2: success — only reached if the feedback retry re-ran the turn.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(
            rig::test_utils::MockError::ProviderResponse(rig::ProviderResponseError::new(
                http::StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
            )),
        )],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
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
    let outcome = result.map_err(|e| format!("feedback retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "feedback retry must complete the turn"
    );
    assert_eq!(
        probe.request_count(),
        2,
        "model-correctable failure must re-run the turn exactly once"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        1,
        "the saved prompt must carry exactly one feedback message; got {persisted:?}"
    );
    let feedback_msg = persisted
        .iter()
        .find(|m| {
            message_text(m).is_some_and(|t| {
                t.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX)
            })
        })
        .ok_or("should have the appended feedback message in the session history")?;
    assert!(
        matches!(feedback_msg, crate::types::Message::User { .. }),
        "appended feedback message must have User role; got {feedback_msg:?}"
    );

    Ok(())
}

/// After a successful feedback retry, the retry prompt carries the feedback
/// message and the saved exchange carries it exactly once.
#[tokio::test]
async fn successful_feedback_retry_delivers_feedback_exactly_once() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-exactly-once";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("recovered".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
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
    let outcome = result.map_err(|e| format!("feedback retry should recover the turn: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "feedback retry must complete the turn"
    );

    // The feedback rides the retry prompt, so the saved user message carries it
    // exactly once.
    let requests = probe.requests();
    assert_eq!(
        requests.len(),
        2,
        "the feedback retry re-runs the turn once"
    );
    assert!(
        request_prompt(&requests[1])
            .starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX),
        "the retry prompt must carry the feedback message"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "the failed attempt's delta, then the completed run's prompt and reply; \
         got {persisted:?}"
    );
    assert_eq!(
        feedback_message_count(&persisted),
        1,
        "the saved prompt carries the feedback message exactly once; got {persisted:?}"
    );
    let reply_count = persisted
        .iter()
        .filter(|m| message_text(m).as_deref() == Some("recovered"))
        .count();
    assert_eq!(
        reply_count, 1,
        "assistant response must be persisted exactly once"
    );

    Ok(())
}

/// A turn with no session (final_session_id None) gets the feedback steering
/// too: the retry prompt carries the feedback message and the second scripted
/// turn completes the run.
#[tokio::test]
async fn no_session_turn_gets_feedback_steering() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let probe = model.clone();
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
            None,
        )
        .await;

    // -- Check
    let outcome = result.map_err(|e| format!("session-less feedback retry must recover: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "session-less turns get the feedback steering too"
    );
    assert_eq!(
        probe.request_count(),
        2,
        "the session-less turn must re-run once via feedback"
    );
    let requests = probe.requests();
    let retry_prompt = request_prompt(&requests[1]);
    assert!(
        retry_prompt.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX),
        "the session-less retry prompt must carry the feedback message; got: {retry_prompt}"
    );

    Ok(())
}
