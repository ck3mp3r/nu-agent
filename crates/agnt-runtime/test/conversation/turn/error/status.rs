use super::support::{turn_error_from_http_status, turn_error_from_http_status_with_msg};
use crate::conversation::turn::executor::CompletionErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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
    let streaming_err = rig::completion::PromptError::Provider(rig::error::ProviderError::Http(
        std::sync::Arc::new(http_err),
    ));
    let kind = match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    };
    assert_eq!(kind, CompletionErrorKind::Network);
    assert!(kind.is_retryable());
}
