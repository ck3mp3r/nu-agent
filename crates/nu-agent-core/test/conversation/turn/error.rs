//! Error-classification unit tests for `TurnError` / `CompletionErrorKind`.

use crate::conversation::turn::executor::{CompletionErrorCategory, CompletionErrorKind};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// an `InvalidStatusCode` HTTP error (numeric status code, no body).
fn turn_error_from_http_status(status: u16) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCode(
        reqwest::StatusCode::from_u16(status).map_err(|e| format!("valid status code: {e:?}"))?,
    );
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// an `InvalidStatusCodeWithMessage` HTTP error (status + body string).
fn turn_error_from_http_status_with_msg(status: u16, body: &str) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCodeWithMessage(
        reqwest::StatusCode::from_u16(status).map_err(|e| format!("valid status code: {e:?}"))?,
        body.to_string(),
    );
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// a `ResponseError` (provider returned a parseable error string).
fn turn_error_from_response_error(msg: &str) -> CompletionErrorKind {
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::ResponseError(msg.to_string()),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<StreamingError>` using
/// a `PromptError::CompletionError` wrapping a `CompletionError`.
fn turn_error_from_prompt_wrapped(
    completion_err: rig::completion::CompletionError,
) -> crate::conversation::turn::TurnError {
    let prompt_err = rig::completion::PromptError::CompletionError(completion_err);
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(prompt_err));
    crate::conversation::turn::TurnError::from(streaming_err)
}

// ---------------------------------------------------------------------------
// Prompt-wrapped provider error classification tests
// ---------------------------------------------------------------------------

/// A `PromptError::CompletionError(ResponseError(s))` must classify like the
/// direct `StreamingError::Completion` path: kind from `classify_from_display`,
/// msg equal to the inner `CompletionError` Display ("ResponseError: {s}").
#[test]
fn prompt_wrapped_response_error_classifies_and_strips_completion_prefix() -> Result<()> {
    let s = "the model produced no answer and stopped with finish_reason=Length; \
             the turn ran out of output budget before producing one — raise max_tokens for this request";
    let turn_err = turn_error_from_prompt_wrapped(rig::completion::CompletionError::ResponseError(
        s.to_string(),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::CompletionError(ResponseError("finish_reason=length"))` must
/// classify as `OutputBudget`.
#[test]
fn prompt_wrapped_finish_reason_length_is_output_budget() -> Result<()> {
    let turn_err = turn_error_from_prompt_wrapped(rig::completion::CompletionError::ResponseError(
        "FinishReasonError { message: finish_reason=length }".to_string(),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::CompletionError(HttpError(InvalidStatusCode(429)))` must
/// classify as `RateLimit`.
#[test]
fn prompt_wrapped_http_429_is_rate_limit() -> Result<()> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCode(reqwest::StatusCode::from_u16(429)?);
    let turn_err =
        turn_error_from_prompt_wrapped(rig::completion::CompletionError::HttpError(http_err));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::MemoryError` must continue to classify as `Unknown`.
#[test]
fn prompt_wrapped_memory_error_is_unknown() -> Result<()> {
    let memory_err = rig::memory::MemoryError::Internal("boom".to_string());
    let prompt_err = rig::completion::PromptError::MemoryError(memory_err);
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(prompt_err));
    let turn_err = crate::conversation::turn::TurnError::from(streaming_err);
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::Unknown);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// `From<PromptError>` must route `PromptError::CompletionError` through the
/// same helper, producing the same kind and msg as the StreamingError path.
#[test]
fn from_prompt_error_completion_error_matches_streaming_path() -> Result<()> {
    let s = "provider exploded";
    let completion_err = rig::completion::CompletionError::ResponseError(s.to_string());
    let prompt_err = rig::completion::PromptError::CompletionError(completion_err);
    let turn_err = crate::conversation::turn::TurnError::from(prompt_err);
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg } => {
            assert_eq!(kind, CompletionErrorKind::Unknown);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Structural HTTP status → error kind classification tests
// ---------------------------------------------------------------------------

/// HTTP 429 with a message body must classify as `RateLimit`.
/// Uses `InvalidStatusCodeWithMessage` (status + body) path.
#[test]
fn http_429_with_message_is_rate_limit() -> Result<()> {
    let kind = turn_error_from_http_status_with_msg(429, "rate_limit_error")?;
    assert_eq!(
        kind,
        CompletionErrorKind::RateLimit,
        "HTTP 429 with message must classify as RateLimit"
    );
    assert!(kind.is_retryable(), "RateLimit must be retryable");
    Ok(())
}

/// Every HTTP status code maps to the correct error kind and retryable flag.
#[test]
fn http_status_to_error_kind() -> Result<()> {
    let cases: &[(u16, CompletionErrorKind, bool)] = &[
        (429, CompletionErrorKind::RateLimit, true),
        (500, CompletionErrorKind::ServerError, true),
        (503, CompletionErrorKind::Overloaded, true),
        (529, CompletionErrorKind::Overloaded, true),
        (504, CompletionErrorKind::ServerError, true),
        (413, CompletionErrorKind::RequestTooLarge, false),
        (401, CompletionErrorKind::Auth, false),
        (403, CompletionErrorKind::Auth, false),
        (402, CompletionErrorKind::Quota, false),
        (404, CompletionErrorKind::EndpointNotFound, false),
        (502, CompletionErrorKind::Unknown, false),
    ];
    for (status, expected_kind, retryable) in cases {
        let kind = turn_error_from_http_status(*status)?;
        assert_eq!(
            kind, *expected_kind,
            "status={status}: expected {expected_kind:?}, got {kind:?}"
        );
        assert_eq!(kind.is_retryable(), *retryable, "retryable status={status}");
    }
    Ok(())
}

/// `StreamEnded` must classify as `Network` (retryable).
#[test]
fn from_streaming_http_stream_ended_is_network() {
    use rig::http_client;
    let http_err = http_client::Error::StreamEnded;
    let streaming_err = rig::agent::StreamingError::Completion(
        rig::completion::CompletionError::HttpError(http_err),
    );
    let kind = match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    };
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}

/// TDD (task step 8): `Instance(Box<dyn Error>)` with display `"error decoding response body"`
/// must classify as `Network` (retryable).  This is the BUG FIX test: the old string-matcher
/// used the pattern `"decode error"` but reqwest's actual Display string is
/// `"error decoding response body"`, so the old code returned `Unknown` (non-retryable).
/// The new `classify_from_display` uses `"error decoding"` which correctly matches.
#[test]
fn from_streaming_http_instance_error_decoding_response_body_is_network() {
    // The concrete type is erased, so we can only test through the Display string.
    // We simulate this via ResponseError (which uses classify_from_display) since we cannot
    // construct http_client::Error::Instance without a real Box<dyn std::error::Error>.
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

/// `"finish_reason=length"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_finish_reason_length_is_output_budget() {
    let kind =
        turn_error_from_response_error("FinishReasonError { message: finish_reason=length }");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// `"ran out of output budget"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_out_of_output_budget_is_output_budget() {
    let kind = turn_error_from_response_error("The model ran out of output budget");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
}

/// `"raise max_tokens"` via ResponseError must classify as `OutputBudget`.
#[test]
fn from_streaming_response_raise_max_tokens_is_output_budget() {
    let kind = turn_error_from_response_error("max_tokens too low, raise max_tokens");
    assert_eq!(kind, CompletionErrorKind::OutputBudget);
    assert!(!kind.is_retryable());
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

/// `PromptCancelled` via StreamingError must produce `Cancelled` variant.
#[test]
fn from_streaming_prompt_cancelled_produces_cancelled_variant() {
    let inner = rig::completion::PromptError::PromptCancelled {
        reason: "cancelled".to_string(),
        chat_history: vec![],
    };
    let streaming_err = rig::agent::StreamingError::Prompt(Box::new(inner));
    let turn_err = crate::conversation::turn::TurnError::from(streaming_err);
    assert!(turn_err.is_cancelled());
}

#[test]
fn is_retryable_matches_spec() {
    // Retryable kinds
    assert!(
        CompletionErrorKind::RateLimit.is_retryable(),
        "RateLimit must be retryable"
    );
    assert!(
        CompletionErrorKind::Overloaded.is_retryable(),
        "Overloaded must be retryable"
    );
    assert!(
        CompletionErrorKind::ServerError.is_retryable(),
        "ServerError must be retryable"
    );
    assert!(
        CompletionErrorKind::Network.is_retryable(),
        "Network must be retryable"
    );

    // Non-retryable kinds
    assert!(
        !CompletionErrorKind::RequestTooLarge.is_retryable(),
        "RequestTooLarge must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ContextOverflow.is_retryable(),
        "ContextOverflow must not be retryable"
    );
    assert!(
        !CompletionErrorKind::ToolStructure.is_retryable(),
        "ToolStructure must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Auth.is_retryable(),
        "Auth must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Quota.is_retryable(),
        "Quota must not be retryable"
    );
    assert!(
        !CompletionErrorKind::CreditsExhausted.is_retryable(),
        "CreditsExhausted must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Refusal.is_retryable(),
        "Refusal must not be retryable"
    );
    assert!(
        !CompletionErrorKind::OutputBudget.is_retryable(),
        "OutputBudget must not be retryable"
    );
    assert!(
        !CompletionErrorKind::EndpointNotFound.is_retryable(),
        "EndpointNotFound must not be retryable"
    );
    assert!(
        !CompletionErrorKind::Unknown.is_retryable(),
        "Unknown must not be retryable"
    );
}

/// Every one of the 14 variants maps to exactly one category from the single
/// classification site (`CompletionErrorKind::category`).
#[test]
fn category_maps_every_variant_to_exactly_one_category() {
    // -- Setup & Fixtures
    let all: &[(CompletionErrorKind, CompletionErrorCategory)] = &[
        (
            CompletionErrorKind::RateLimit,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::Overloaded,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::ServerError,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::Network,
            CompletionErrorCategory::Retryable,
        ),
        (
            CompletionErrorKind::OutputBudget,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::ToolStructure,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::RequestTooLarge,
            CompletionErrorCategory::Steerable,
        ),
        (
            CompletionErrorKind::ContextOverflow,
            CompletionErrorCategory::HardStop,
        ),
        (CompletionErrorKind::Auth, CompletionErrorCategory::HardStop),
        (
            CompletionErrorKind::Quota,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::CreditsExhausted,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::Refusal,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::EndpointNotFound,
            CompletionErrorCategory::HardStop,
        ),
        (
            CompletionErrorKind::Unknown,
            CompletionErrorCategory::HardStop,
        ),
    ];

    // -- Exec & Check
    for (kind, expected) in all {
        assert_eq!(
            kind.category(),
            *expected,
            "{kind:?} must map to {expected:?}"
        );
    }
}

/// "error sending request" must be classified as `Network` (retryable).
/// Tests the `classify_from_display` path for the `Instance` case.
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
