//! `TurnError` — sum-type error for a single conversation turn.
//!
//! Replaces the old struct that used a `cancelled: bool` flag and an
//! `Option<Vec<Message>>` to distinguish error categories.  Consumers now
//! pattern-match on variants instead of interrogating boolean fields.
//!
//! # `CompletionErrorKind` derivation
//!
//! `CompletionFailed::kind` is set at the `From<StreamingError>` boundary by
//! matching **structurally** on `rig::completion::CompletionError` variants —
//! no post-hoc string parsing.  String matching is used only as a last resort for
//! `http_client::Error::Instance(Box<dyn Error>)` and for `ResponseError`/`ProviderError`
//! payloads where the concrete type has been erased by the provider SDK.

use crate::conversation::turn::executor::CompletionErrorKind;
use crate::types::Message;

// ---------------------------------------------------------------------------
// TurnContext
// ---------------------------------------------------------------------------

/// Caller-local context available in `execute_turn` but not
/// at the `From<>` boundary. Passed alongside `TurnError` to the executor.
#[derive(Debug)]
pub(crate) struct TurnContext {
    pub last_known_history: Vec<Message>,
    pub pre_turn_message_count: usize,
}

// ---------------------------------------------------------------------------
// TurnError
// ---------------------------------------------------------------------------

/// Error produced by a single conversation turn.
#[derive(Debug, Clone)]
pub enum TurnError {
    /// User or hook cancelled the turn mid-flight.
    /// `messages` is rig's full `chat_history` from `PromptCancelled`.
    Cancelled {
        /// Human-readable reason supplied by the hook.
        msg: String,
        /// Full rig `chat_history` captured at the cancellation point.
        messages: Vec<Message>,
    },

    /// The LLM exceeded the configured tool-turn limit.
    /// `messages` is rig's full `chat_history` from `MaxTurnsError`.
    MaxTurnsExceeded {
        /// Formatted error message including the limit.
        msg: String,
        /// Configured maximum turns that was exceeded.
        max_turns: usize,
        /// Full rig `chat_history` at the limit point.
        messages: Vec<Message>,
    },

    /// The LLM called a tool that is not in the registry.
    /// `messages` is rig's full `chat_history` from `UnknownToolCall`.
    UnknownTool {
        /// Formatted error message including the tool name.
        msg: String,
        /// Name of the tool that was not found.
        tool_name: String,
        /// Full rig `chat_history` at the error point.
        messages: Vec<Message>,
    },

    /// HTTP / transport / decode failure from the LLM provider.
    ///
    /// `kind` is pre-classified at the `From<StreamingError>` boundary by
    /// matching on `rig::completion::CompletionError` variants structurally.
    CompletionFailed {
        /// Display message from the underlying rig error.
        msg: String,
        /// Pre-classified error category (set structurally, not via string matching).
        kind: CompletionErrorKind,
    },

    /// Tool execution infrastructure failure (`ToolError` / `ToolSetError`).
    ToolExecutionFailed {
        /// Display message from the underlying error.
        msg: String,
    },
}

impl TurnError {
    /// Returns `true` for the `Cancelled` variant.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { msg, .. } => write!(f, "Cancelled: {msg}"),
            Self::MaxTurnsExceeded { msg, .. }
            | Self::UnknownTool { msg, .. }
            | Self::CompletionFailed { msg, .. }
            | Self::ToolExecutionFailed { msg, .. } => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TurnError {}

// ---------------------------------------------------------------------------
// From<PromptError>
// ---------------------------------------------------------------------------

impl From<rig::completion::PromptError> for TurnError {
    fn from(err: rig::completion::PromptError) -> Self {
        match err {
            rig::completion::PromptError::PromptCancelled {
                reason,
                chat_history,
            } => Self::Cancelled {
                msg: reason,
                messages: chat_history,
            },
            rig::completion::PromptError::MaxTurnsError {
                max_turns,
                chat_history,
                ..
            } => Self::MaxTurnsExceeded {
                msg: format!("Max turns ({max_turns}) exceeded"),
                max_turns,
                messages: *chat_history,
            },
            rig::completion::PromptError::UnknownToolCall {
                tool_name,
                chat_history,
                ..
            } => Self::UnknownTool {
                msg: format!("Unknown tool: {tool_name}"),
                tool_name,
                messages: *chat_history,
            },
            other => Self::CompletionFailed {
                msg: other.to_string(),
                kind: CompletionErrorKind::Unknown,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// From<StreamingError>
// ---------------------------------------------------------------------------

/// Classify a status code into a `CompletionErrorKind`.
fn classify_by_status(status: u16) -> CompletionErrorKind {
    match status {
        429 => CompletionErrorKind::RateLimit,
        529 | 503 => CompletionErrorKind::Overloaded,
        500 | 504 => CompletionErrorKind::ServerError,
        413 => CompletionErrorKind::RequestTooLarge,
        401 | 403 => CompletionErrorKind::Auth,
        402 => CompletionErrorKind::Quota,
        404 => CompletionErrorKind::EndpointNotFound,
        _ => CompletionErrorKind::Unknown,
    }
}

/// Classify a string (from `ResponseError`, `ProviderError`, or an erased `Instance` error)
/// into a `CompletionErrorKind`.
///
/// This function is used when the concrete error type is unavailable and only the Display
/// string is accessible.  Structural matching (via HTTP status codes) is always preferred
/// and is done first in `From<StreamingError>`.  This function acts as the fallback for
/// `Instance(Box<dyn Error>)`, `ResponseError`, and `ProviderError` payloads.
///
/// Patterns are ordered most-specific first to avoid misclassification.
fn classify_from_display(msg: &str) -> CompletionErrorKind {
    let lower = msg.to_lowercase();

    // ToolStructure — most specific (400 + tool keyword)
    if (lower.contains("invalid_request_body") || lower.contains("invalid_request_error"))
        && (lower.contains("tool_use")
            || lower.contains("tool_result")
            || lower.contains("call_id")
            || lower.contains("function_call"))
    {
        return CompletionErrorKind::ToolStructure;
    }

    // ContextOverflow — key patterns from provider error messages
    if lower.contains("context_length_exceeded")
        || lower.contains("context length exceeded")
        || lower.contains("exceeds the context window")
        || lower.contains("prompt is too long")
        || lower.contains("input is too long for requested model")
        || lower.contains("input token count")
        || lower.contains("maximum context length")
        || lower.contains("max tokens")
        || lower.contains("token limit")
        || lower.contains("context window is full")
        || lower.contains("context window exceeded")
        || lower.contains("reduce the length")
        || lower.contains("too many tokens")
        || lower.contains("string too long")
    {
        return CompletionErrorKind::ContextOverflow;
    }

    // RequestTooLarge
    if lower.contains("request_too_large")
        || lower.contains("request entity too large")
        || contains_status_token(msg, "413")
    {
        return CompletionErrorKind::RequestTooLarge;
    }

    // Refusal
    if lower.contains("content_policy")
        || lower.contains("content policy")
        || lower.contains("safety")
        || lower.contains("moderation")
        || lower.contains("refusal")
        || lower.contains("refused")
    {
        return CompletionErrorKind::Refusal;
    }

    // CreditsExhausted
    if lower.contains("out of credits") || lower.contains("top up") {
        return CompletionErrorKind::CreditsExhausted;
    }

    // Quota / billing (check before CreditsExhausted "credits" substring)
    if contains_status_token(msg, "402")
        || lower.contains("billing_error")
        || lower.contains("billing")
        || lower.contains("quota")
        || lower.contains("insufficient_quota")
    {
        return CompletionErrorKind::Quota;
    }

    // RateLimit
    if lower.contains("rate_limit")
        || lower.contains("rate limit")
        || contains_status_token(msg, "429")
    {
        return CompletionErrorKind::RateLimit;
    }

    // Overloaded
    if lower.contains("overloaded")
        || contains_status_token(msg, "529")
        || contains_status_token(msg, "503")
    {
        return CompletionErrorKind::Overloaded;
    }

    // ServerError
    if contains_status_token(msg, "500")
        || lower.contains("api_error")
        || contains_status_token(msg, "504")
        || lower.contains("timeout_error")
    {
        return CompletionErrorKind::ServerError;
    }

    // Network — retryable transport/decode errors
    if lower.contains("error sending request") {
        return CompletionErrorKind::Network;
    }
    if lower.contains("error decoding") {
        // Matches "error decoding response body" — the bug fix vs old "decode error" pattern
        return CompletionErrorKind::Network;
    }
    if lower.contains("connection reset") || lower.contains("connection refused") {
        return CompletionErrorKind::Network;
    }
    if lower.contains("network error") {
        return CompletionErrorKind::Network;
    }
    // Note: old pattern was "decode error" — misses "error decoding response body".
    // We keep "decode error" here for backward compatibility but the primary fix
    // is "error decoding" above.
    if lower.contains("decode error") {
        return CompletionErrorKind::Network;
    }
    if lower.contains("invalid utf-8") || lower.contains("unexpected eof") {
        return CompletionErrorKind::Network;
    }

    // EndpointNotFound
    if contains_status_token(msg, "404")
        || lower.contains("not_found_error")
        || lower.contains("endpoint not found")
    {
        return CompletionErrorKind::EndpointNotFound;
    }

    // Auth
    if contains_status_token(msg, "401")
        || lower.contains("authentication_error")
        || contains_status_token(msg, "403")
        || lower.contains("permission_error")
    {
        return CompletionErrorKind::Auth;
    }

    CompletionErrorKind::Unknown
}

/// Returns `true` if `code` appears as a standalone token in `msg`.
///
/// Splits on whitespace and checks each word after stripping leading/trailing
/// non-digit punctuation. This prevents false-positives like `"5000 tokens"`
/// matching `"500"` or `"step 4042"` matching `"404"`.
fn contains_status_token(msg: &str, code: &str) -> bool {
    msg.split_whitespace()
        .any(|word| word.trim_matches(|c: char| !c.is_ascii_digit()) == code)
}

impl From<rig::agent::StreamingError> for TurnError {
    fn from(e: rig::agent::StreamingError) -> Self {
        use rig::completion::CompletionError;
        use rig::http_client;

        match e {
            rig::agent::StreamingError::Prompt(boxed) => match *boxed {
                rig::completion::PromptError::PromptCancelled {
                    reason,
                    chat_history,
                } => Self::Cancelled {
                    msg: reason,
                    messages: chat_history,
                },
                rig::completion::PromptError::MaxTurnsError {
                    max_turns,
                    chat_history,
                    ..
                } => Self::MaxTurnsExceeded {
                    msg: format!("Max turns ({max_turns}) exceeded"),
                    max_turns,
                    messages: *chat_history,
                },
                rig::completion::PromptError::UnknownToolCall {
                    tool_name,
                    chat_history,
                    ..
                } => Self::UnknownTool {
                    msg: format!("Unknown tool: {tool_name}"),
                    tool_name,
                    messages: *chat_history,
                },
                other => Self::CompletionFailed {
                    msg: other.to_string(),
                    kind: CompletionErrorKind::Unknown,
                },
            },

            rig::agent::StreamingError::Completion(completion_err) => {
                let msg = completion_err.to_string();
                match completion_err {
                    CompletionError::HttpError(http_err) => {
                        let kind = match http_err {
                            http_client::Error::InvalidStatusCode(s) => {
                                classify_by_status(s.as_u16())
                            }
                            http_client::Error::InvalidStatusCodeWithMessage(s, _) => {
                                classify_by_status(s.as_u16())
                            }
                            http_client::Error::StreamEnded => CompletionErrorKind::Network,
                            http_client::Error::Instance(_) => {
                                // reqwest::Error erased to Box<dyn Error> — use Display
                                classify_from_display(&http_err.to_string())
                            }
                            _ => CompletionErrorKind::Unknown,
                        };
                        Self::CompletionFailed { msg, kind }
                    }
                    CompletionError::ResponseError(s) => Self::CompletionFailed {
                        kind: classify_from_display(&s),
                        msg,
                    },
                    CompletionError::ProviderError(s) => Self::CompletionFailed {
                        kind: classify_from_display(&s),
                        msg,
                    },
                    CompletionError::RequestError(s) => Self::CompletionFailed {
                        kind: classify_from_display(&s.to_string()),
                        msg,
                    },
                    // rig 0.42.0 surfaces a failed streaming handshake (connect-time
                    // non-success, e.g. a 500) as `ProviderResponse` carrying the
                    // status and body, instead of `HttpError(InvalidStatusCodeWithMessage)`.
                    // Classify by status when present so a 500/503/429 stays retryable.
                    CompletionError::ProviderResponse(e) => {
                        let kind = match e.status {
                            Some(status) => classify_by_status(status.as_u16()),
                            None => classify_from_display(&e.body),
                        };
                        Self::CompletionFailed { msg, kind }
                    }
                    _ => Self::CompletionFailed {
                        msg,
                        kind: CompletionErrorKind::Unknown,
                    },
                }
            }
        }
    }
}
