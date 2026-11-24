//! `TurnError` — sum-type error for a single conversation turn.
//!
//! Replaces the old struct that used a `cancelled: bool` flag and an
//! `Option<Vec<Message>>` to distinguish error categories.  Consumers now
//! pattern-match on variants instead of interrogating boolean fields.
//!
//! # `CompletionErrorKind` derivation
//!
//! `CompletionFailed::kind` is set at the `From<PromptError>` boundary by
//! matching **structurally** on `rig::error::ProviderError` variants —
//! no post-hoc string parsing.  String matching is used only as a last resort for
//! `http_client::Error::Permanent(Box<dyn Error>)` and for `ProviderError::Response` /
//! `ProviderError::Provider` payloads where the concrete type has been erased by the
//! provider SDK.

use std::time::{Duration, SystemTime};

use rig::completion::FinishReason;

use crate::conversation::turn::executor::CompletionErrorKind;
use crate::hook::PromptAnchor;
use crate::hook::turn_tools::TurnToolRecord;
use crate::types::Message;

use super::end::TurnExhaustion;

// ---------------------------------------------------------------------------
// TurnContext
// ---------------------------------------------------------------------------

/// Caller-local context available in `execute_turn` but not
/// at the `From<>` boundary. Passed alongside `TurnError` to the executor.
#[derive(Debug)]
pub(crate) struct TurnContext {
    pub last_known_history: Vec<Message>,
    /// Number of messages the session's rig log held when the turn started.
    /// Its only role is the skip guard: a log whose length moved since the turn
    /// started is left alone.
    pub pre_turn_message_count: usize,
    /// Where the prompt the turn dispatched sits in `last_known_history`, as the
    /// run recorded it when it started. Recovery starts the turn's delta there,
    /// so an identical earlier prompt cannot move the anchor and a rewritten
    /// prompt cannot invalidate it. `None` when no run dispatched a prompt.
    pub prompt_anchor: Option<PromptAnchor>,
    /// Every tool call the turn dispatched and the result each call settled
    /// with. A stop drops the in-flight batch from `last_known_history`, so
    /// this is the only record of those calls.
    pub tool_records: Vec<TurnToolRecord>,
}

// ---------------------------------------------------------------------------
// TurnError
// ---------------------------------------------------------------------------

/// Error produced by a single conversation turn.
#[derive(Debug, Clone)]
pub enum TurnError {
    /// User or hook cancelled the turn mid-flight.
    /// `messages` is rig's full `chat_history` from `PromptError::Cancelled`.
    Cancelled {
        /// Human-readable reason supplied by the hook.
        msg: String,
        /// Full rig `chat_history` captured at the cancellation point.
        messages: Vec<Message>,
    },

    /// The LLM exceeded the configured tool-turn limit.
    /// `messages` is rig's full `chat_history` from `PromptError::MaxTurns`.
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
    /// `kind` is pre-classified at the `From<PromptError>` boundary by
    /// matching on `rig::error::ProviderError` variants structurally, and on
    /// the typed finish reason the failing completion reported.
    CompletionFailed {
        /// Display message from the underlying rig error.
        msg: String,
        /// Pre-classified error category (set structurally, not via string matching).
        kind: CompletionErrorKind,
        /// The wait the provider asked for, read once at this boundary through
        /// [`rig::error::ErrorReport::retry_after`].
        retry_after: Option<Duration>,
        /// What the retry/steering loop exhausted before the failure became
        /// terminal. `None` when the first attempt was terminal.
        exhaustion: Option<TurnExhaustion>,
    },

    /// Tool execution infrastructure failure (`ToolHandlerError` / `ToolSetError`).
    ToolExecutionFailed {
        /// Display message from the underlying error.
        msg: String,
    },

    /// The session's store refused the database, so this turn could persist
    /// nothing. The turn fails on the refusal text instead of running without
    /// memory and reporting success.
    StoreRefused {
        /// The refusal text the store reported.
        msg: String,
    },
}

impl TurnError {
    /// Returns `true` for the `Cancelled` variant.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Convert the failure of a streamed run, reading the typed reason its last
    /// completion stopped on.
    ///
    /// rig reports why a reply ended on the completion call it surfaced before
    /// the failure; a reply the output limit cut produced no answer, so its
    /// failure is the turn running out of output budget. `finish` is `None`
    /// when the stream reported no such call, and every failure then reads its
    /// own status and display text.
    pub(crate) fn from_failure(
        err: rig::completion::PromptError,
        finish: Option<&FinishReason>,
    ) -> Self {
        match err {
            rig::completion::PromptError::Cancelled {
                reason,
                chat_history,
            } => Self::Cancelled {
                msg: reason,
                messages: chat_history,
            },
            rig::completion::PromptError::MaxTurns {
                max_turns,
                chat_history,
                ..
            } => Self::MaxTurnsExceeded {
                msg: format!("Max turns ({max_turns}) exceeded"),
                max_turns,
                messages: chat_history,
            },
            rig::completion::PromptError::UnknownToolCall {
                tool_name,
                chat_history,
                ..
            } => Self::UnknownTool {
                msg: format!("Unknown tool: {tool_name}"),
                tool_name,
                messages: chat_history,
            },
            rig::completion::PromptError::Provider(e) => completion_error_to_turn_error(e, finish),
            // Conversation memory failed to load or persist history.
            rig::completion::PromptError::Memory { error, .. } => Self::CompletionFailed {
                msg: error.to_string(),
                kind: CompletionErrorKind::Unknown,
                retry_after: None,
                exhaustion: None,
            },
            // A relayed report preserves the origin's status and message.
            rig::completion::PromptError::Report(report) => {
                let kind = classify_report(&report);
                let retry_after = retry_wait(&report);
                Self::CompletionFailed {
                    msg: report.message.clone(),
                    kind,
                    retry_after,
                    exhaustion: None,
                }
            }
            other => Self::CompletionFailed {
                msg: other.to_string(),
                kind: CompletionErrorKind::Unknown,
                retry_after: None,
                exhaustion: None,
            },
        }
    }
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { msg, .. } => write!(f, "Cancelled: {msg}"),
            Self::MaxTurnsExceeded { msg, .. }
            | Self::UnknownTool { msg, .. }
            | Self::CompletionFailed { msg, .. }
            | Self::ToolExecutionFailed { msg }
            | Self::StoreRefused { msg } => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TurnError {}

impl From<rig::completion::PromptError> for TurnError {
    fn from(err: rig::completion::PromptError) -> Self {
        Self::from_failure(err, None)
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify a `rig::error::ProviderError` into a `TurnError::CompletionFailed`.
///
/// Shared by the `From<PromptError>` boundary so provider failures wrapped as
/// `PromptError::Provider` reach the same classification as the direct
/// `PromptError::Report` path. `finish` is the typed reason the turn's last
/// completion stopped on, when the stream reported one: a reply the output
/// limit cut is a cut-off reply, whatever its diagnostic says.
fn completion_error_to_turn_error(
    err: rig::error::ProviderError,
    finish: Option<&FinishReason>,
) -> TurnError {
    use rig::error::ProviderError;
    use rig::http_client;

    let msg = err.to_string();
    let retry_after = retry_wait(&err.report());
    let kind = match err {
        ProviderError::Http(http_err) => match &*http_err {
            http_client::Error::InvalidStatusCodeWithDetails { status, .. } => {
                classify_by_status(status.as_u16())
            }
            http_client::Error::StreamEnded => CompletionErrorKind::Network,
            http_client::Error::Transient(_) => CompletionErrorKind::Network,
            http_client::Error::Permanent(source) => {
                // The transport error erased to Box<dyn Error> — use Display
                classify_text(&source.to_string(), finish)
            }
            _ => CompletionErrorKind::Unknown,
        },
        ProviderError::Response(s) => classify_text(&s, finish),
        ProviderError::Provider(s) => classify_text(&s, finish),
        ProviderError::Request(s) => classify_from_display(&s.to_string()),
        // rig surfaces a failed streaming handshake (connect-time
        // non-success, e.g. a 500) as `ProviderResponse` carrying the
        // status and body, instead of `Http(InvalidStatusCodeWithDetails)`.
        // Classify by status when present so a 500/503/429 stays retryable.
        ProviderError::ProviderResponse(e) => match e.status {
            Some(status) => classify_by_status(status.as_u16()),
            None => classify_text(&e.body, finish),
        },
        // The provider rejected the configured credentials with 401 or 403.
        ProviderError::InvalidAuthentication(_) => CompletionErrorKind::Auth,
        // The reply stopped before the provider ended it — a transport fault.
        ProviderError::Truncated => CompletionErrorKind::Network,
        // The driver dropped a streamed reply that sent nothing for its idle
        // window. Upstream classes this as a timeout and retries it; our
        // timeout-class kind is `ServerError`, which is retryable.
        ProviderError::Stalled(_) => CompletionErrorKind::ServerError,
        // A relayed report preserves the origin's status and message.
        ProviderError::Relayed(report) => classify_report(&report),
        _ => CompletionErrorKind::Unknown,
    };
    TurnError::CompletionFailed {
        msg,
        kind,
        retry_after,
        exhaustion: None,
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify a rig `ErrorReport` into a `CompletionErrorKind`.
///
/// The report's HTTP status decides when it names a family of its own; a status
/// the table does not know (a plain 400, which is how a provider reports a
/// context overflow) leaves the decision to the message's display text. Without
/// a status the text decides outright.
pub(crate) fn classify_report(report: &rig::error::ErrorReport) -> CompletionErrorKind {
    match report.http_status {
        Some(status) => match classify_by_status(status) {
            CompletionErrorKind::Unknown => classify_from_display(&report.message),
            kind => kind,
        },
        None => classify_from_display(&report.message),
    }
}

/// Classify display text, letting the typed finish reason decide first.
///
/// A reply the output limit cut is read from that typed reason, never from the
/// diagnostic rig writes about it: the display text is content for the user.
fn classify_text(msg: &str, finish: Option<&FinishReason>) -> CompletionErrorKind {
    finish
        .and_then(classify_finish)
        .unwrap_or_else(|| classify_from_display(msg))
}

/// The kind a reply's typed finish reason decides, when it decides one.
///
/// The output limit is the only reason the turn cannot absorb: the reply it cut
/// produced no answer, so the turn failed on its budget. Every other reason
/// leaves the failure to its own status and display text.
fn classify_finish(finish: &FinishReason) -> Option<CompletionErrorKind> {
    match finish {
        FinishReason::Length => Some(CompletionErrorKind::OutputBudget),
        _ => None,
    }
}

/// The wait `report` asks for, as of now, through rig's own hint reader.
///
/// Read once at the classification boundary: the retry loop holds the kind and
/// this wait, never the report, so no path re-parses a hint out of display text.
fn retry_wait(report: &rig::error::ErrorReport) -> Option<Duration> {
    report.retry_after(SystemTime::now())
}

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

/// Classify a string (from `ResponseError`, `ProviderError`, or an erased `Permanent` error)
/// into a `CompletionErrorKind`.
///
/// This function is used when the concrete error type is unavailable and only the Display
/// string is accessible.  Structural matching (via HTTP status codes) is always preferred
/// and is done first in `From<PromptError>`.  This function acts as the fallback for
/// `Permanent(Box<dyn Error>)`, `ResponseError`, and `ProviderError` payloads.
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

    // OutputBudget from provider wording — run before ContextOverflow; the
    // cut-off reply itself is read from the typed finish reason in
    // `classify_finish`, never from a diagnostic string.
    if lower.contains("exceeds model's maximum output tokens")
        || (lower.contains("exceeds") && lower.contains("output tokens"))
    {
        return CompletionErrorKind::OutputBudget;
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
        // Matches "error decoding response body".
        return CompletionErrorKind::Network;
    }
    if lower.contains("connection reset") || lower.contains("connection refused") {
        return CompletionErrorKind::Network;
    }
    if lower.contains("network error") {
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

#[cfg(test)]
#[path = "../../../test/conversation/turn/error/support.rs"]
mod support;

#[cfg(test)]
#[path = "../../../test/conversation/turn/error/boundary.rs"]
mod boundary_test;

#[cfg(test)]
#[path = "../../../test/conversation/turn/error/status.rs"]
mod status_test;

#[cfg(test)]
#[path = "../../../test/conversation/turn/error/classify.rs"]
mod classify_test;

#[cfg(test)]
#[path = "../../../test/conversation/turn/error/kind.rs"]
mod kind_test;
