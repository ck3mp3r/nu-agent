use super::*;

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
    // Every attempt's delta is persisted: the three feedback retries persist
    // theirs before re-running, and the capped final attempt persists its own.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        4,
        "one delta per attempt: three feedback retries plus the capped final attempt; \
         got {persisted:?}"
    );
    assert!(
        message_text(&persisted[0]).is_some_and(|text| text.ends_with("hello")),
        "the persisted delta keeps the original prompt; got {persisted:?}"
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
    assert_eq!(
        persisted.len(),
        1,
        "a failed turn persists its delta; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("hello"));

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
    assert_eq!(
        persisted.len(),
        4,
        "one delta per attempt: two feedback retries, the backoff retry and the final \
         failed attempt; got {persisted:?}"
    );
    assert!(
        message_text(&persisted[0]).is_some_and(|text| text.ends_with("hello")),
        "the persisted delta keeps the original prompt; got {persisted:?}"
    );

    Ok(())
}
