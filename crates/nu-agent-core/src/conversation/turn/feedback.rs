//! Model-facing feedback for provider-failed turns.
//!
//! Pure functions used by the turn-executor wiring: decide whether a
//! completion error kind is correctable by the model, and build the neutral
//! feedback message the model sees on its next attempt. This module owns no
//! executor state and performs no I/O.

use super::executor::CompletionErrorKind;

/// Neutral frame prepended to every feedback message.
///
/// Deliberately free of blame or permission language: the model reads this on
/// its next attempt and must act on the remedy, not on speculation about the
/// operator's setup.
pub const FEEDBACK_PREFIX: &str = "The previous turn attempt failed at the provider.";

/// Maximum bytes of the raw provider message kept in the feedback.
const FEEDBACK_RAW_MSG_MAX_BYTES: usize = 500;

/// Returns `true` for error kinds the model can correct on its next attempt.
///
/// Retryable kinds (rate limit, overload, server error, network) are handled
/// by the executor's retry loop and never reach the feedback path. Permanent
/// infrastructure failures (auth, quota, credits, refusal, endpoint, unknown)
/// need operator action, so they are not model-correctable either.
pub fn is_model_correctable(kind: &CompletionErrorKind) -> bool {
    matches!(
        kind,
        CompletionErrorKind::ToolStructure
            | CompletionErrorKind::ContextOverflow
            | CompletionErrorKind::OutputBudget
            | CompletionErrorKind::RequestTooLarge
    )
}

/// Build the model-facing feedback message for a provider-failed turn.
///
/// The message starts with [`FEEDBACK_PREFIX`], carries a model-actionable
/// remedy for the kind (worded for the model, unlike the user-facing
/// `kind_to_user_msg`), and embeds the raw provider message cut to
/// [`FEEDBACK_RAW_MSG_MAX_BYTES`] at a valid UTF-8 char boundary.
pub fn build_feedback_message(kind: &CompletionErrorKind, raw_provider_msg: &str) -> String {
    let remedy = match kind {
        CompletionErrorKind::ToolStructure => "ensure every tool call has a matching tool result",
        CompletionErrorKind::ContextOverflow => {
            "the conversation is too long; continue with a shorter request"
        }
        CompletionErrorKind::OutputBudget => {
            "the response hit the output token limit; continue with a shorter response"
        }
        CompletionErrorKind::RequestTooLarge => {
            "the request was too large; reduce the size of attached tool results"
        }
        // Non-model-correctable kinds still get the neutral frame; the wiring
        // only feeds model-correctable kinds back to the model.
        _ => "wait for the operator to resolve the underlying issue",
    };
    if raw_provider_msg.is_empty() {
        format!("{FEEDBACK_PREFIX} Remedy: {remedy}.")
    } else {
        let cut = cut_at_char_boundary(raw_provider_msg, FEEDBACK_RAW_MSG_MAX_BYTES);
        format!("{FEEDBACK_PREFIX} Remedy: {remedy}. Raw provider message: {cut}")
    }
}

// region:    --- Support

/// Cut `s` to at most `max_bytes`, backing off to a valid UTF-8 char boundary.
fn cut_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

// endregion: --- Support

#[cfg(test)]
#[path = "feedback_test.rs"]
mod tests;
