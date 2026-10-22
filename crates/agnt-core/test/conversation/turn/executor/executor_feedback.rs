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

/// Four consecutive model-correctable failures with a cap of
/// MAX_PROVIDER_FEEDBACK_RETRIES (3) must produce exactly three feedback
/// messages and then fall through to the hard-error path.
///
/// The carrier is replaced, never accumulated: retry 1 carries the First
/// remedy, retries 2 and 3 carry the Backoff remedy and none of the First
/// remedy's text. Under accumulation retry 2 would carry both.
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

    // Each retry's prompt carries the feedback message above the original
    // prompt; the failed run saves nothing.
    let requests = probe.requests();
    assert_eq!(request_prompt(&requests[0]), "hello");
    for (i, request) in requests.iter().enumerate().skip(1) {
        let prompt = request_prompt(request);
        assert!(
            prompt.starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX),
            "retry {i} must carry the feedback message; got: {prompt}"
        );
        assert!(
            prompt.ends_with("hello"),
            "retry {i} must keep the original prompt; got: {prompt}"
        );
    }
    // Latest-wins: the carrier is replaced, never accumulated. Retry 1 carries
    // the First remedy; retries 2 and 3 carry the Backoff remedy and none of
    // the First remedy's distinctive text. Under accumulation retry 2 would
    // carry both.
    let first_prompt = request_prompt(&requests[1]);
    assert!(
        first_prompt.contains(crate::config::defaults::DEFAULT_OUTPUT_BUDGET_EMPTY_REMEDY),
        "retry 1 must carry the First remedy; got: {first_prompt}"
    );
    for (i, request) in requests.iter().enumerate().skip(2) {
        let prompt = request_prompt(request);
        assert!(
            prompt.contains("still hitting the output token limit"),
            "retry {i} must carry the Backoff remedy; got: {prompt}"
        );
        assert!(
            !prompt.contains("before any answer"),
            "retry {i} must not carry the First remedy (latest-wins, not accumulation); got: {prompt}"
        );
    }
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a failed turn saves nothing; got {persisted:?}"
    );

    Ok(())
}

/// A retryable-kind failure (429 RateLimit) must get no feedback steering and
/// must not consume the feedback budget.
#[tokio::test]
async fn retryable_kind_failure_gets_no_feedback_steering() -> Result<()> {
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

    let requests = probe.requests();
    assert_eq!(
        request_prompt(&requests[0]),
        "hello",
        "retryable kinds must not steer the prompt"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a failed turn saves nothing; got {persisted:?}"
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
        2,
        "the completed run saves the prompt and the reply; got {persisted:?}"
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

    // The two model-correctable failures steer the next attempt; the 429
    // backoff retry reuses the carrier.
    let requests = probe.requests();
    let steered = requests
        .iter()
        .filter(|request| {
            request_prompt(request)
                .starts_with(crate::conversation::turn::feedback::FEEDBACK_PREFIX)
        })
        .count();
    assert_eq!(
        steered, 3,
        "the two feedback retries and the backoff retry carry the feedback message"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a failed turn saves nothing; got {persisted:?}"
    );

    Ok(())
}
