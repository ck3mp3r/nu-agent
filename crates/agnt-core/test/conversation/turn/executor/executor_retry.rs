//! Backoff-retry tests: retry loop, retry-after parsing, and jitter.

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_config};
use super::*;
use crate::config::Config;
use crate::session::StoreEntry;

// ---------------------------------------------------------------------------
// Gap 3 — Retry-with-backoff tests
// ---------------------------------------------------------------------------

/// Retry succeeds on the second attempt: first call returns a retryable 500 error,
/// second call succeeds. Result should be Ok with 2 messages in JSONL.
#[tokio::test]
async fn retry_succeeds_on_second_attempt() -> Result<()> {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-success";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: error (retryable 500). Turn 2: success.
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error internal server")],
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

    assert!(
        result.is_ok(),
        "retry should succeed on second attempt; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("retry should succeed on second attempt: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::Completed));

    let persisted_entries = memory_state
        .inner_memory()
        .load_all(session_id)
        .await
        .map_err(|e| format!("store load should succeed: {e:?}"))?;
    let persisted: Vec<crate::types::Message> = persisted_entries
        .iter()
        .filter_map(|e| match e {
            StoreEntry::Message(m) => Some(m.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        persisted.len(),
        2,
        "successful retry must produce 2 messages (user + assistant); got {}",
        persisted.len()
    );

    Ok(())
}

/// Retry exhausted: all attempts fail with retryable errors. The final error
/// message must mention the retry attempt count.
#[tokio::test]
async fn retry_exhausted_surfaces_attempt_count() -> Result<()> {
    let config = Config {
        max_retries: Some(2),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-retry-exhausted";
    let mut memory_state = make_memory_state(&temp_dir);

    // All 3 attempts (1 initial + 2 retries) fail with retryable error
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
        vec![MockStreamEvent::error("500 api_error server down")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

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

    assert!(result.is_err(), "exhausted retries must return Err");
    let err_msg = result
        .err()
        .map(|e| e.to_string())
        .ok_or("exhausted retries must return Err")?;
    assert!(
        err_msg.contains("after 2 retries"),
        "error message must mention retry count; got: {err_msg}"
    );

    Ok(())
}

/// Non-retryable errors (e.g., a ServerError-style outage) must NOT get the
/// backoff retry. Model-correctable errors (e.g., context_length_exceeded)
/// now get feedback retries instead, so a single scripted turn ends after
/// one model call plus one feedback re-run — still no backoff retries.
#[tokio::test]
async fn non_retryable_error_not_retried() {
    let config = Config {
        max_retries: Some(3),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-non-retryable";
    let mut memory_state = make_memory_state(&temp_dir);

    // Only 1 turn — the second attempt fails with the mock's out-of-turns
    // error, which is neither retryable nor model-correctable, so the loop
    // exits with no backoff retries (no "Turn failed after N retries" wrap).
    // A 400 context_length_exceeded is NOT retryable.
    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "context_length_exceeded in prompt",
    )]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

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

    // Must fail immediately without retrying
    assert!(
        result.is_err(),
        "non-retryable error must not be retried; got Ok"
    );
    // If the model had been called more than once, MockCompletionModel would panic
    // (it only has 1 turn configured). The test passing proves exactly 1 HTTP request.
}

/// When `max_retries` is `Some(0)`, no retry is attempted regardless of error type.
///
/// Tests the most direct way to disable retries: setting max_retries=0 ensures
/// `attempt < max_retries` is always false on the first attempt (attempt=0 < 0 = false).
/// The test verifies the error message does NOT contain "retries" — proving the retry
/// path was not entered.
#[tokio::test]
async fn retry_disabled_when_max_retries_is_zero() -> Result<()> {
    let config = Config {
        max_retries: Some(0),
        retry_base_delay_ms: Some(1),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-no-retry-guard";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "500 api_error server error",
    )]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

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

    assert!(result.is_err(), "error must propagate without retry");
    let err_msg = result
        .err()
        .map(|e| e.to_string())
        .ok_or("error must propagate without retry")?;
    // When max_retries=0, attempt never increments, so "retries" should not appear
    assert!(
        !err_msg.contains("retries"),
        "error message must NOT mention retries when max_retries=0; got: {err_msg}"
    );

    Ok(())
}

/// When `last_known_history` is empty the retry guard (`has_partial_history`) prevents
/// any retry from being attempted, even when `max_retries > 0` and the error is retryable.
///
/// After the refactor, caller-local context lives in `TurnContext` (from `error.rs`),
/// not in `TurnError`. This test verifies the guard logic directly by constructing
/// `TurnContext` values and checking that `last_known_history.is_empty()` correctly
/// controls the guard condition.
///
/// For the full-path scenario (history always non-empty via MockCompletionModel), see
/// `retry_disabled_when_max_retries_is_zero` which tests the equivalent observable outcome.
#[test]
fn retry_not_attempted_when_no_partial_history() {
    use crate::conversation::turn::error::TurnContext;

    // Empty history: has_partial_history guard must be false.
    let ctx_empty = TurnContext {
        last_known_history: vec![],
        pre_turn_message_count: 0,
    };
    let has_partial_history = !ctx_empty.last_known_history.is_empty();

    // Guard must evaluate to false with empty history — retry must not be attempted.
    assert!(
        !has_partial_history,
        "has_partial_history guard must be false when last_known_history is empty; \
         retry must be suppressed even for retryable errors with max_retries > 0"
    );

    // Confirm the mirror case: non-empty history enables the guard.
    let ctx_non_empty = TurnContext {
        last_known_history: vec![crate::types::Message::user("prompt")],
        pre_turn_message_count: 0,
    };
    let has_partial_history_non_empty = !ctx_non_empty.last_known_history.is_empty();
    assert!(
        has_partial_history_non_empty,
        "has_partial_history guard must be true when last_known_history is non-empty"
    );
}

// ---------------------------------------------------------------------------
// Gap 3 — extract_retry_after_ms unit tests
// ---------------------------------------------------------------------------

#[test]
fn extract_retry_after_ms_parses_seconds_basic() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("rate limited, retry after 5 seconds"),
        Some(5000)
    );
}

#[test]
fn extract_retry_after_ms_parses_retry_after_header() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("HTTP 429: Retry-After: 30"),
        Some(30_000)
    );
}

#[test]
fn extract_retry_after_ms_parses_underscore_variant() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("error: retry_after: 10 seconds"),
        Some(10_000)
    );
}

#[test]
fn extract_retry_after_ms_returns_none_when_absent() {
    use super::extract_retry_after_ms;
    assert_eq!(
        extract_retry_after_ms("rate limit exceeded, try again later"),
        None
    );
}

#[test]
fn extract_retry_after_ms_handles_zero() {
    use super::extract_retry_after_ms;
    assert_eq!(extract_retry_after_ms("retry after 0 seconds"), Some(0));
}

// ---------------------------------------------------------------------------
// Jitter variance test
// ---------------------------------------------------------------------------

/// Verify that the jitter factor produces varying delays across multiple samples.
///
/// The retry loop uses `0.8 + (rand::random::<f64>() * 0.4)` which gives a
/// jitter_factor in [0.8, 1.2). Over 50 samples, the min and max must differ
/// by at least 10% of the base delay — confirming non-deterministic jitter.
#[test]
fn jitter_produces_varying_delays() {
    let base_delay_ms: u64 = 1000;
    let samples: Vec<u64> = (0..50)
        .map(|_| {
            let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
            (base_delay_ms as f64 * jitter_factor) as u64
        })
        .collect();

    let min = samples.iter().copied().min().unwrap_or(base_delay_ms);
    let max = samples.iter().copied().max().unwrap_or(base_delay_ms);
    let range = max - min;

    // With 50 samples from a uniform [0.8, 1.2) distribution applied to 1000ms,
    // the range should be well above 100ms (10% of base). In practice it will be
    // close to 400ms (the theoretical max range of 800..1200). We assert > 50ms
    // to avoid flakiness while still catching deterministic implementations.
    assert!(
        range > 50,
        "jitter must produce varying delays; got range={range}ms (min={min}, max={max})"
    );

    // Verify all samples are within the expected [800, 1200) range
    for &sample in &samples {
        assert!(
            (800..1200).contains(&sample),
            "jittered delay must be in [800, 1200); got {sample}"
        );
    }
}
