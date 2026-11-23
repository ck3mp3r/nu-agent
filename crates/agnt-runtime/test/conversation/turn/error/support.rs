//! Error-classification unit tests for `TurnError` / `CompletionErrorKind`.

use crate::conversation::turn::executor::CompletionErrorKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper: build a `TurnError::CompletionFailed` via `From<PromptError>` using
/// an `InvalidStatusCodeWithDetails` HTTP error (numeric status code, no body).
pub(super) fn turn_error_from_http_status(status: u16) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCodeWithDetails {
        status: reqwest::StatusCode::from_u16(status)
            .map_err(|e| format!("valid status code: {e:?}"))?,
        body: String::new(),
        headers: http::HeaderMap::new(),
    };
    let streaming_err = rig::completion::PromptError::Provider(rig::error::ProviderError::Http(
        std::sync::Arc::new(http_err),
    ));
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<PromptError>` using
/// an `InvalidStatusCodeWithDetails` HTTP error (status + body string).
pub(super) fn turn_error_from_http_status_with_msg(
    status: u16,
    body: &str,
) -> Result<CompletionErrorKind> {
    use rig::http_client;
    let http_err = http_client::Error::InvalidStatusCodeWithDetails {
        status: reqwest::StatusCode::from_u16(status)
            .map_err(|e| format!("valid status code: {e:?}"))?,
        body: body.to_string(),
        headers: http::HeaderMap::new(),
    };
    let streaming_err = rig::completion::PromptError::Provider(rig::error::ProviderError::Http(
        std::sync::Arc::new(http_err),
    ));
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => Ok(kind),
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<PromptError>` using
/// a `ProviderError::Response` (provider returned a parseable error string).
pub(super) fn turn_error_from_response_error(msg: &str) -> CompletionErrorKind {
    let streaming_err = rig::completion::PromptError::Provider(
        rig::error::ProviderError::Response(msg.to_string()),
    );
    match crate::conversation::turn::TurnError::from(streaming_err) {
        crate::conversation::turn::TurnError::CompletionFailed { kind, .. } => kind,
        other => panic!("expected CompletionFailed, got: {other:?}"),
    }
}

/// Helper: build a `TurnError::CompletionFailed` via `From<PromptError>` using
/// a `PromptError::CompletionError` wrapping a `ProviderError`.
pub(super) fn turn_error_from_prompt_wrapped(
    completion_err: rig::error::ProviderError,
) -> crate::conversation::turn::TurnError {
    let prompt_err = rig::completion::PromptError::Provider(completion_err);

    crate::conversation::turn::TurnError::from(prompt_err)
}
