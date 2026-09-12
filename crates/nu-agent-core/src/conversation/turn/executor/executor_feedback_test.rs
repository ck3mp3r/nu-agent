//! Provider-feedback retry tests (model-correctable completion errors).

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_config};
use super::*;
use crate::config::Config;

// ---------------------------------------------------------------------------
// Provider feedback retry (model-correctable completion errors)
// ---------------------------------------------------------------------------

/// A model-correctable provider failure classified by HTTP status (413 →
/// RequestTooLarge via `classify_by_status`) must append exactly one user-role
/// feedback message to the session memory and re-run the turn — proven by a
/// second scripted turn succeeding.
#[tokio::test]
async fn model_correctable_failure_appends_feedback_and_reruns_turn() -> Result<()> {
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
        "exactly one feedback message must be appended; got {persisted:?}"
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

/// Four consecutive model-correctable failures with a cap of
/// MAX_PROVIDER_FEEDBACK_RETRIES (3) must produce exactly three feedback
/// messages and then fall through to the hard-error path.
#[tokio::test]
async fn feedback_cap_three_produces_three_messages_then_hard_error() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-cap";
    let mut memory_state = make_memory_state(&temp_dir);

    let overflow = || MockStreamEvent::error("The model ran out of output budget");
    let model = MockCompletionModel::from_stream_turns([
        vec![overflow()],
        vec![overflow()],
        vec![overflow()],
        vec![overflow()],
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
    let err = result
        .err()
        .ok_or("capped model-correctable failures must fall through to the hard error")?;
    assert_eq!(
        probe.request_count(),
        4,
        "three feedback retries plus the capped final attempt = 4 model calls"
    );
    assert!(
        !err.msg.contains("Turn failed after"),
        "feedback retries must not increment the backoff attempt counter; got: {}",
        err.msg
    );
    assert!(
        err.msg.contains("max_output_tokens"),
        "hard-error message must describe the final OutputBudget failure; got: {}",
        err.msg
    );
    assert!(
        err.msg
            .contains(crate::conversation::turn::executor::NO_OUTPUT_STATEMENT),
        "exhausted steering must surface the no-output statement; got: {}",
        err.msg
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        3,
        "cap 3 must produce exactly three feedback messages; got {persisted:?}"
    );

    Ok(())
}

/// A retryable-kind failure (429 RateLimit) must append no feedback message
/// and must not consume the feedback budget.
#[tokio::test]
async fn retryable_kind_failure_appends_no_feedback_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-retryable-no-feedback";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "429 rate_limit_error",
    )]]);
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
    assert!(
        result.is_err(),
        "retryable error with max_retries=0 must fail the turn"
    );
    assert_eq!(
        probe.request_count(),
        1,
        "retryable kinds must not trigger a feedback retry"
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        0,
        "retryable kinds must not append feedback messages; got {persisted:?}"
    );

    Ok(())
}

/// After a successful feedback retry, the session store must contain the
/// feedback message exactly once and the turn's exchange exactly once.
#[tokio::test]
async fn successful_feedback_retry_persists_feedback_exactly_once() -> Result<()> {
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

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "store must hold feedback + user prompt + assistant response; got {persisted:?}"
    );
    assert_eq!(
        feedback_message_count(&persisted),
        1,
        "feedback message must be persisted exactly once; got {persisted:?}"
    );
    let prompt_count = persisted
        .iter()
        .filter(|m| message_text(m).as_deref() == Some("hello"))
        .count();
    assert_eq!(
        prompt_count, 1,
        "user prompt must be persisted exactly once"
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

/// A turn with no session (final_session_id None) must not append feedback
/// messages: the second scripted turn stays unconsumed, proving the feedback
/// branch never fired.
#[tokio::test]
async fn no_session_turn_appends_no_feedback_message() -> Result<()> {
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
    let err = result
        .err()
        .ok_or("session-less model-correctable failure must return Err")?;
    assert_eq!(
        probe.request_count(),
        1,
        "session-less turns must not re-run via feedback"
    );
    assert!(
        err.msg.contains("max_output_tokens"),
        "hard-error message must describe the OutputBudget failure; got: {}",
        err.msg
    );

    Ok(())
}

/// Two feedback retries followed by a 429 failure must still get the backoff
/// retry: the feedback budget and the backoff budget are independent.
#[tokio::test]
async fn feedback_retries_do_not_consume_backoff_budget() -> Result<()> {
    // -- Setup & Fixtures
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-feedback-then-backoff";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("The model ran out of output budget")],
        vec![MockStreamEvent::error("429 rate_limit_error")],
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
    let err = result
        .err()
        .ok_or("turn must fail after the mock runs out of scripted turns")?;
    assert_eq!(
        probe.request_count(),
        4,
        "2 feedback retries + 429 backoff retry + final failed attempt = 4 model calls"
    );
    assert!(
        err.msg.contains("after 1 retries"),
        "the 429 must consume the backoff budget (attempt 1), not the feedback budget; got: {}",
        err.msg
    );

    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        feedback_message_count(&persisted),
        2,
        "only the two model-correctable failures append feedback; got {persisted:?}"
    );

    Ok(())
}
