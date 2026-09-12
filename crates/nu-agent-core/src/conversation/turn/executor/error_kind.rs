/// Describes the category of a completion failure.
///
/// Used by callers to decide whether to retry, surface a user-readable message,
/// or trigger session repair.  The enum makes these decisions explicit rather
/// than embedding policy in string matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionErrorKind {
    /// 429 — temporary rate limit; retryable.
    RateLimit,
    /// 529/503 — provider overloaded; retryable.
    Overloaded,
    /// 500/504 — provider server error; retryable.
    ServerError,
    /// Transport or stream-decode error; retryable.
    Network,
    /// 413 — single request too large; permanent.
    RequestTooLarge,
    /// 400 context_length_exceeded — conversation too long; permanent.
    ContextOverflow,
    /// Output budget exhausted — response cut short at max_output_tokens; permanent.
    OutputBudget,
    /// 400 with tool_use/tool_result — malformed tool sequence; permanent.
    ToolStructure,
    /// 401/403 — authentication or permission failure; permanent.
    Auth,
    /// 402 — billing limit; permanent.
    Quota,
    /// Credits exhausted on provider account; permanent.
    CreditsExhausted,
    /// Content policy or safety refusal; permanent.
    Refusal,
    /// 404 — endpoint not found; permanent.
    EndpointNotFound,
    /// Unrecognised error; treat as non-retryable until classified.
    Unknown,
}

impl CompletionErrorKind {
    /// Classify this kind into exactly one category.
    ///
    /// This is the single classification site for all 14 variants. Callers
    /// decide their response from the category, never from scattered matches
    /// over individual kinds.
    pub fn category(&self) -> CompletionErrorCategory {
        match self {
            Self::RateLimit | Self::Overloaded | Self::ServerError | Self::Network => {
                CompletionErrorCategory::Retryable
            }
            Self::OutputBudget | Self::ToolStructure | Self::RequestTooLarge => {
                CompletionErrorCategory::Steerable
            }
            Self::ContextOverflow
            | Self::Auth
            | Self::Quota
            | Self::CreditsExhausted
            | Self::Refusal
            | Self::EndpointNotFound
            | Self::Unknown => CompletionErrorCategory::HardStop,
        }
    }

    /// Returns `true` for transient errors that are safe to retry.
    pub fn is_retryable(&self) -> bool {
        self.category() == CompletionErrorCategory::Retryable
    }
}

/// The category of a completion failure, deciding how the executor responds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionErrorCategory {
    /// Transient infrastructure failure — retry with backoff.
    Retryable,
    /// Model-correctable failure — steer with escalating feedback.
    Steerable,
    /// Permanent failure — kill the turn immediately.
    HardStop,
}

/// Returns a user-visible error message for a given `CompletionErrorKind`.
///
/// The message describes what went wrong in human-readable terms. Called by the
/// hard-error path in `TurnExecutor::execute` to produce the `LabeledError` returned
/// to the Nushell caller.
pub(super) fn kind_to_user_msg(kind: &CompletionErrorKind, raw_msg: &str) -> String {
    match kind {
        CompletionErrorKind::ToolStructure => {
            "Turn failed: the API rejected this turn — a tool call was missing its result. The session has been repaired.".to_string()
        }
        CompletionErrorKind::ContextOverflow => {
            "Turn failed: conversation too long. Run 'agent session compact' to summarise."
                .to_string()
        }
        CompletionErrorKind::OutputBudget => {
            "Turn failed: output budget exhausted. Increase max_output_tokens in config or pass --max-output-tokens."
                .to_string()
        }
        CompletionErrorKind::RequestTooLarge => {
            "Turn failed: tool results too large. Lower max_tool_result_bytes in config."
                .to_string()
        }
        CompletionErrorKind::Refusal => {
            "Turn failed: the provider refused this request (content policy or safety filter)."
                .to_string()
        }
        CompletionErrorKind::CreditsExhausted => {
            "Turn failed: account credits exhausted. Top up your provider account.".to_string()
        }
        CompletionErrorKind::Quota => {
            "Turn failed: billing limit reached. Check your provider account.".to_string()
        }
        CompletionErrorKind::RateLimit => "Turn failed: rate limit reached.".to_string(),
        CompletionErrorKind::Overloaded => "Turn failed: provider overloaded.".to_string(),
        CompletionErrorKind::ServerError => "Turn failed: provider server error.".to_string(),
        CompletionErrorKind::Network => "Turn failed: network error.".to_string(),
        CompletionErrorKind::EndpointNotFound => {
            "Turn failed: API endpoint not found. Check your provider configuration.".to_string()
        }
        CompletionErrorKind::Auth => {
            "Turn failed: authentication failed. Check your API key.".to_string()
        }
        CompletionErrorKind::Unknown => format!("Turn failed: {raw_msg}"),
    }
}
