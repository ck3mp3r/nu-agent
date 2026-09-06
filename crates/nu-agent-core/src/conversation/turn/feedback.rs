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

/// Neutral frame prepended to every max-turns steering message.
///
/// Distinct from [`FEEDBACK_PREFIX`]: exhausting the tool-call budget is not a
/// provider failure, so the message frames the exhaustion as a steering
/// opportunity rather than an error report.
pub const MAX_TURNS_FEEDBACK_PREFIX: &str =
    "The previous turn attempt used all its tool-call turns.";

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
///
/// For `OutputBudget`, the remedy is configurable: `output_budget_remedy_mode`
/// selects between "empty_output" (default) and "shorter_response". In
/// "empty_output" mode, `output_budget_empty_remedy` overrides the built-in
/// default text when set.
pub fn build_feedback_message(
    kind: &CompletionErrorKind,
    raw_provider_msg: &str,
    output_budget_empty_remedy: Option<&str>,
    output_budget_remedy_mode: Option<&str>,
) -> String {
    let remedy = match kind {
        CompletionErrorKind::ToolStructure => "ensure every tool call has a matching tool result",
        CompletionErrorKind::ContextOverflow => {
            "the conversation is too long; continue with a shorter request"
        }
        CompletionErrorKind::OutputBudget => {
            output_budget_remedy(output_budget_empty_remedy, output_budget_remedy_mode)
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

/// Build the model-facing steering message for an exhausted tool-call budget.
///
/// The message starts with [`MAX_TURNS_FEEDBACK_PREFIX`], names the exhausted
/// budget, challenges the model to question its tool-call volume and
/// reconsider its approach, and states that a fresh budget is available if it
/// changes course. Worded for the model, never the user.
pub fn build_max_turns_feedback_message(max_turns: usize) -> String {
    format!(
        "{MAX_TURNS_FEEDBACK_PREFIX} The budget was {max_turns} tool-call turns. \
        Question whether the task truly needs that many tool calls. \
        Reconsider your approach and aim to accomplish the goal with fewer, \
        larger steps. If you change course, a fresh budget of {max_turns} turns \
        is available."
    )
}

// region:    --- Support

/// Select the OutputBudget remedy text from config.
///
/// `output_budget_remedy_mode` selects the mode: "empty_output" (default)
/// steers the model to produce its answer immediately; "shorter_response"
/// keeps the legacy shorten-the-response guidance. In "empty_output" mode,
/// `output_budget_empty_remedy` overrides the built-in default when set.
fn output_budget_remedy<'a>(
    output_budget_empty_remedy: Option<&'a str>,
    output_budget_remedy_mode: Option<&str>,
) -> &'a str {
    let mode = output_budget_remedy_mode
        .unwrap_or(crate::config::defaults::DEFAULT_OUTPUT_BUDGET_REMEDY_MODE);
    match mode {
        "shorter_response" => {
            "the response hit the output token limit; continue with a shorter response"
        }
        // "empty_output" (default) or any unset/unknown value
        _ => output_budget_empty_remedy
            .unwrap_or(crate::config::defaults::DEFAULT_OUTPUT_BUDGET_EMPTY_REMEDY),
    }
}

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
