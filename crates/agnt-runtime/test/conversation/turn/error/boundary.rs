use super::support::turn_error_from_prompt_wrapped;
use crate::conversation::turn::executor::CompletionErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Prompt-wrapped provider error classification tests
// ---------------------------------------------------------------------------

/// A `PromptError::Provider(ProviderError::Response(s))` must classify like the
/// direct `PromptError::Provider` path: kind from `classify_from_display`,
/// msg equal to the inner `ProviderError` Display ("ResponseError: {s}").
#[test]
fn prompt_wrapped_response_error_classifies_and_strips_completion_prefix() -> Result<()> {
    let s = "rate_limit exceeded";
    let turn_err =
        turn_error_from_prompt_wrapped(rig::error::ProviderError::Response(s.to_string()));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg, .. } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::Provider(ProviderError::Response(...))` from a reply the
/// output limit cut must classify as `OutputBudget` through the typed reason.
#[test]
fn prompt_wrapped_finish_reason_length_is_output_budget() -> Result<()> {
    // -- Setup & Fixtures
    let prompt_err = crate::conversation::turn::TurnError::from_failure(
        rig::completion::PromptError::Provider(rig::error::ProviderError::Response(
            "the model produced no answer".to_string(),
        )),
        Some(&rig::completion::FinishReason::Length),
    );

    // -- Check
    match prompt_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A Go-style wait hint (`try again in 6m0s`) in the preserved provider body is
/// read by `ErrorReport::retry_after`, and the classified failure carries it.
#[test]
fn from_streaming_report_reads_a_go_style_wait_hint() -> Result<()> {
    // -- Setup & Fixtures
    let response = rig::ProviderResponseError::new(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        "{\"error\":\"rate limited, try again in 6m0s\"}",
    );
    let mut report =
        rig::error::ErrorReport::new(rig::error::ErrorKind::ProviderResponse, "rate limited")
            .with_http_status(429)
            .with_retryable(true);
    report.provider_response = Some(response);

    // -- Exec
    let turn_err = crate::conversation::turn::TurnError::from_failure(
        rig::completion::PromptError::Report(report),
        None,
    );

    // -- Check
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed {
            kind, retry_after, ..
        } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
            assert_eq!(
                retry_after,
                Some(core::time::Duration::from_secs(360)),
                "the Go-style hint names a six-minute wait"
            );
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `PromptError::Provider(ProviderError::Http(InvalidStatusCodeWithDetails { 429 }))`
/// must classify as `RateLimit`.
#[test]
fn prompt_wrapped_http_429_is_rate_limit() -> Result<()> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCodeWithDetails {
        status: reqwest::StatusCode::from_u16(429)?,
        body: String::new(),
        headers: http::HeaderMap::new(),
    };
    let turn_err = turn_error_from_prompt_wrapped(rig::error::ProviderError::Http(
        std::sync::Arc::new(http_err),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A `ProviderError::Stalled(idle)` — the driver dropped a streamed reply that
/// sent nothing for its idle window — must classify as `ServerError`
/// (retryable), the same kind our display path gives a "timeout_error".
#[test]
fn prompt_wrapped_stalled_timeout_is_server_error() -> Result<()> {
    let turn_err = turn_error_from_prompt_wrapped(rig::error::ProviderError::Stalled(
        std::time::Duration::from_secs(5),
    ));
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg, .. } => {
            assert_eq!(kind, CompletionErrorKind::ServerError);
            assert!(kind.is_retryable(), "Stalled must be retryable");
            assert!(
                msg.contains("TimeoutError"),
                "msg keeps the provider Display text, got: {msg}"
            );
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// `From<PromptError>` must route `PromptError::Provider` through the
/// same helper, producing the same kind and msg as the PromptError path.
#[test]
fn from_prompt_error_completion_error_matches_streaming_path() -> Result<()> {
    let s = "provider exploded";
    let completion_err = rig::error::ProviderError::Response(s.to_string());
    let prompt_err = rig::completion::PromptError::Provider(completion_err);
    let turn_err = crate::conversation::turn::TurnError::from(prompt_err);
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, msg, .. } => {
            assert_eq!(kind, CompletionErrorKind::Unknown);
            assert_eq!(msg, format!("ResponseError: {s}"));
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A reply the output limit cut is an output-budget turn, read from the typed
/// finish reason the stream reported for the completion it failed on.
#[test]
fn from_streaming_finish_reason_length_is_output_budget() -> Result<()> {
    // -- Setup & Fixtures
    let cut_reply = rig::completion::PromptError::Provider(rig::error::ProviderError::Response(
        "the model produced no answer and stopped with a cut reply".to_string(),
    ));

    // -- Exec
    let turn_err = crate::conversation::turn::TurnError::from_failure(
        cut_reply,
        Some(&rig::completion::FinishReason::Length),
    );

    // -- Check
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::OutputBudget);
            assert!(!kind.is_retryable());
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// A finish reason that cut nothing — a natural stop — leaves the failure to
/// its own status and display text.
#[test]
fn from_streaming_natural_finish_leaves_the_text_to_decide() -> Result<()> {
    // -- Setup & Fixtures
    let failed = rig::completion::PromptError::Provider(rig::error::ProviderError::Response(
        "rate_limit exceeded".to_string(),
    ));

    // -- Exec
    let turn_err = crate::conversation::turn::TurnError::from_failure(
        failed,
        Some(&rig::completion::FinishReason::Stop),
    );

    // -- Check
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// The wait a failing report asks for is read through rig's own hint reader at
/// the classification boundary, so the retry loop never parses display text.
#[test]
fn from_streaming_report_carries_the_wait_it_asked_for() -> Result<()> {
    // -- Setup & Fixtures
    let report = rig::error::ErrorReport::new(rig::error::ErrorKind::ProviderResponse, "slow down")
        .with_http_status(429)
        .with_retryable(true);

    // -- Exec
    let turn_err = crate::conversation::turn::TurnError::from_failure(
        rig::completion::PromptError::Report(report),
        None,
    );

    // -- Check
    match turn_err {
        crate::conversation::turn::TurnError::CompletionFailed {
            kind, retry_after, ..
        } => {
            assert_eq!(kind, CompletionErrorKind::RateLimit);
            assert_eq!(
                retry_after, None,
                "a report with no header and no hint asks for no wait"
            );
        }
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
    Ok(())
}

/// `Cancelled` via PromptError must produce `Cancelled` variant.
#[test]
fn from_streaming_prompt_cancelled_produces_cancelled_variant() {
    let prompt_err = rig::completion::PromptError::Cancelled {
        reason: "cancelled".to_string(),
        chat_history: vec![],
    };
    let turn_err = crate::conversation::turn::TurnError::from(prompt_err);
    assert!(turn_err.is_cancelled());
}

/// WHEN a relayed report carries a status the table does not know and a
/// context-length message, THEN THE `TurnError::from` boundary SHALL answer
/// `ContextOverflow`, through `PromptError::Report` and through
/// `PromptError::Provider(ProviderError::Relayed(_))` alike. A status the table
/// maps keeps its own kind.
#[test]
fn classify_relayed_report_status_then_display_reaches_the_turn_boundary() -> Result<()> {
    use crate::conversation::turn::TurnError;
    use rig::error::{ErrorKind, ErrorReport, ProviderError};

    // -- Setup & Fixtures
    let mut overflow = ErrorReport::new(ErrorKind::ProviderResponse, "context_length_exceeded");
    overflow.http_status = Some(400);
    let mut rate_limited = ErrorReport::new(ErrorKind::ProviderResponse, "slow down");
    rate_limited.http_status = Some(429);

    // -- Exec & Check
    for (report, expected) in [
        (overflow, CompletionErrorKind::ContextOverflow),
        (rate_limited, CompletionErrorKind::RateLimit),
    ] {
        let direct = TurnError::from(rig::completion::PromptError::Report(report.clone()));
        let relayed = TurnError::from(rig::completion::PromptError::Provider(
            ProviderError::Relayed(Box::new(report)),
        ));
        for (turn_err, path) in [
            (direct, "PromptError::Report"),
            (relayed, "ProviderError::Relayed"),
        ] {
            match turn_err {
                TurnError::CompletionFailed { kind, .. } => assert_eq!(
                    kind, expected,
                    "{path} must read a report's status then its display"
                ),
                other => panic!("expected CompletionFailed for {path}, got: {other:?}"),
            }
        }
    }
    Ok(())
}
