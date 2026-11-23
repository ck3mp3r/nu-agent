use super::support::turn_error_from_response_error;
use crate::conversation::turn::executor::CompletionErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The display string `"error decoding response body"` must classify as `Network` (retryable).
/// This is the BUG FIX test: the old string-matcher used the pattern `"decode error"` but
/// reqwest's actual Display string is `"error decoding response body"`, so the old code
/// returned `Unknown` (non-retryable).  The display fallback `classify_from_display` uses
/// `"error decoding"` which correctly matches.
#[test]
fn from_streaming_response_error_decoding_response_body_is_network() {
    // The transport error is erased, so the classification runs on its Display string.
    // This drives that path via ResponseError, which classifies through classify_from_display.
    let kind = turn_error_from_response_error("error decoding response body");
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "'error decoding response body' must classify as Network (retryable), not Unknown"
    );
    assert!(
        kind.is_retryable(),
        "Network must be retryable — this is the bug fix"
    );
}

/// `"error sending request for url ..."` must classify as `Network` via display matching.
#[test]
fn from_streaming_response_error_sending_request_is_network() {
    let kind =
        turn_error_from_response_error("error sending request for url https://api.example.com");
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// `"connection reset by peer"` must classify as `Network`.
#[test]
fn from_streaming_response_connection_reset_is_network() {
    let kind = turn_error_from_response_error("connection reset by peer");
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// `"context_length_exceeded"` via ResponseError must classify as `ContextOverflow`.
#[test]
fn from_streaming_response_context_length_exceeded_is_context_overflow() {
    let kind = turn_error_from_response_error("context_length_exceeded in prompt");
    assert_eq!(kind, CompletionErrorKind::ContextOverflow);
    assert!(!kind.is_retryable());
}

/// `"rate_limit"` via ResponseError must classify as `RateLimit`.
#[test]
fn from_streaming_response_rate_limit_string_is_rate_limit() {
    let kind = turn_error_from_response_error("rate_limit exceeded");
    assert_eq!(kind, CompletionErrorKind::RateLimit);
    assert!(kind.is_retryable());
}

/// Unknown error string via ResponseError falls through to `Unknown`.
#[test]
fn from_streaming_response_unknown_string_is_unknown() {
    let kind = turn_error_from_response_error("502 bad gateway proxy error");
    assert_eq!(kind, CompletionErrorKind::Unknown);
    assert!(!kind.is_retryable());
}

/// The diagnostic rig writes about an answerless cut reply carries no kind of
/// its own: without the typed reason the failure stays `Unknown`.
///
/// The display patterns that used to read it are deleted; the text is content.
/// The fixture is rig's own formatter for the cut reply, so it cannot drift from
/// the wording the provider path actually carries.
#[test]
fn from_streaming_cut_reply_text_alone_is_unknown() {
    let diagnostic = rig::completion::FinishReason::Length.no_answer_message();
    let kind = turn_error_from_response_error(&diagnostic);
    assert_eq!(kind, CompletionErrorKind::Unknown);
}

/// Provider "exceeds model's maximum output tokens" 400 via ResponseError must
/// classify as `OutputBudget` (residual defense for wrong-cache failures).
#[test]
fn from_streaming_response_exceeds_max_output_tokens_is_output_budget() {
    let kind = turn_error_from_response_error(
        "status 400: max_tokens (1048576) exceeds model's maximum output tokens (65536)",
    );
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// WHEN an `ErrorReport` carries a 400 and a context-length message, THEN THE
/// `classify_report` function SHALL answer `ContextOverflow`: the settle site
/// that records a refused request reads the same two steps as the prompt-error
/// boundary, so the overflow verdict cannot disagree with the turn's own
/// classification.
#[test]
fn classify_report_reads_the_status_then_the_display() -> Result<()> {
    use crate::conversation::turn::error::classify_report;
    use rig::error::{ErrorKind, ErrorReport};

    // -- Setup & Fixtures: a provider reply that names the context limit.
    let mut report = ErrorReport::new(ErrorKind::ProviderResponse, "context_length_exceeded");
    report.http_status = Some(400);

    // -- Exec
    let kind = classify_report(&report);

    // -- Check
    assert_eq!(
        kind,
        CompletionErrorKind::ContextOverflow,
        "a 400 naming the context limit classifies as an overflow"
    );

    // -- Exec & Check: a status-less report falls back to its message, and a
    // status that is not an overflow keeps its own classification.
    let mut statusless = ErrorReport::new(ErrorKind::Provider, "prompt is too long");
    statusless.http_status = None;
    assert_eq!(
        classify_report(&statusless),
        CompletionErrorKind::ContextOverflow,
        "without a status the message decides"
    );
    let mut rate_limited = ErrorReport::new(ErrorKind::ProviderResponse, "slow down");
    rate_limited.http_status = Some(429);
    assert_eq!(
        classify_report(&rate_limited),
        CompletionErrorKind::RateLimit,
        "a 429 is not an overflow"
    );
    Ok(())
}

/// "error sending request" must be classified as `Network` (retryable).
/// Tests the `classify_from_display` path for the `Permanent` case.
#[test]
fn classify_error_sending_request_is_network() {
    let kind = turn_error_from_response_error(
        "error sending request for url https://api.example.com/v1/chat",
    );
    assert_eq!(
        kind,
        CompletionErrorKind::Network,
        "'error sending request' must be Network"
    );
    assert!(kind.is_retryable(), "Network must be retryable");
}
