//! Repairs a turn's message delta before it is persisted.
//!
//! rig 0.45 shapes the request history itself (`pairing::pair`) and repairs the
//! store on read, so the host keeps two repair concerns: closing the tool calls
//! a cancelled or failed turn never answered, and replacing rig's placeholder
//! result with the outcome the turn settled.

use rig::conversation::pairing::NO_RESULT_PROVIDED;

use crate::types::{
    AssistantContent, CallId, Message, ToolName, ToolResult, ToolResultContent, UserContent,
};

/// The content a closed call carries when the turn never settled it.
pub(crate) const INTERRUPTED_RESULT: &str = "[interrupted]";

/// The prefix every permission-denial result text carries.
const DENIED_RESULT_PREFIX: &str = "Permission denied";

/// Whether `result_text` is a persisted not-run row: a call the turn never
/// ran, as opposed to a settled success or failure.
///
/// Three shapes qualify: rig's placeholder for a call nothing answered, the
/// host's marker for a call killed mid-run, and a permission denial.
pub(crate) fn is_did_not_run_result(result_text: &str) -> bool {
    result_text == NO_RESULT_PROVIDED
        || result_text == INTERRUPTED_RESULT
        || result_text.starts_with(DENIED_RESULT_PREFIX)
}

/// Closes every `Assistant(ToolCall)` in `messages` that no `User(ToolResult)`
/// answers: one synthetic `User` message holding one `ToolResult` per unpaired
/// call is inserted right after that assistant message.
///
/// `results` holds `(call id, result text, is_error)` for the calls whose
/// outcome the turn observed. A call answered by `results` closes with that
/// text and error flag; every other unpaired call closes with
/// [`INTERRUPTED_RESULT`].
///
/// The delta a cancelled or failed turn leaves behind is rig-committed history:
/// structurally sound except for the calls the turn never answered. This closes
/// those calls so the saved conversation replays on the next turn.
pub(crate) fn close_unanswered_tool_calls_with(
    messages: Vec<Message>,
    results: &[(CallId, String, bool)],
) -> Vec<Message> {
    use std::collections::HashSet;

    // Every call id a result already answers, anywhere in the history.
    let answered: HashSet<CallId> = messages
        .iter()
        .flat_map(|msg| match msg {
            Message::User { content } => content
                .iter()
                .filter_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr.call.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();

    let mut result: Vec<Message> = Vec::with_capacity(messages.len() + 4);
    let mut closed: usize = 0;

    for msg in messages {
        let unanswered: Vec<(CallId, ToolName)> = match &msg {
            Message::Assistant(assistant) => assistant
                .content
                .iter()
                .filter_map(|item| match item {
                    AssistantContent::ToolCall(tc) if !answered.contains(&tc.id) => {
                        Some((tc.id.clone(), tc.function.name.clone()))
                    }
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };

        result.push(msg);

        if unanswered.is_empty() {
            continue;
        }

        let content: Vec<UserContent> = unanswered
            .into_iter()
            .map(|(call, name)| {
                closed += 1;
                let settled = results.iter().find(|(id, ..)| *id == call);
                let (text, is_error) = match settled {
                    Some((_, text, is_error)) => (text.as_str(), *is_error),
                    None => (INTERRUPTED_RESULT, false),
                };
                UserContent::ToolResult(ToolResult {
                    call,
                    name,
                    content: vec![ToolResultContent::text(text)],
                    is_error,
                })
            })
            .collect();
        result.push(Message::User { content });
    }

    if closed > 0 {
        log::debug!("close_unanswered_tool_calls_with: inserted {closed} synthetic results");
    }

    result
}

/// Replaces rig's placeholder result with the outcome the turn settled.
///
/// rig closes a call nothing answered with [`NO_RESULT_PROVIDED`] and
/// `is_error: true`. When the turn observed that call's outcome, the delta
/// carries both the placeholder and the record; this pass writes the record's
/// text and error flag over the placeholder, so the saved conversation holds
/// the real result. A placeholder no record matches is left alone, as is every
/// other result.
///
/// `results` holds `(call id, result text, is_error)` for the calls whose
/// outcome the turn observed, the same slice the close pass takes.
pub(crate) fn replace_placeholder_results_with(
    messages: Vec<Message>,
    results: &[(CallId, String, bool)],
) -> Vec<Message> {
    let mut replaced: usize = 0;

    let messages = messages
        .into_iter()
        .map(|msg| match msg {
            Message::User { content } => Message::User {
                content: content
                    .into_iter()
                    .map(|item| match item {
                        UserContent::ToolResult(result) if is_placeholder(&result) => {
                            match results.iter().find(|(id, ..)| *id == result.call) {
                                Some((_, text, is_error)) => {
                                    replaced += 1;
                                    UserContent::ToolResult(ToolResult {
                                        content: vec![ToolResultContent::text(text)],
                                        is_error: *is_error,
                                        ..result
                                    })
                                }
                                None => UserContent::ToolResult(result),
                            }
                        }
                        other => other,
                    })
                    .collect(),
            },
            other => other,
        })
        .collect();

    if replaced > 0 {
        log::debug!("replace_placeholder_results_with: replaced {replaced} placeholder results");
    }

    messages
}

// region:    --- Support

/// Whether `result` is rig's placeholder: one text block holding
/// [`NO_RESULT_PROVIDED`], flagged as an error.
fn is_placeholder(result: &ToolResult) -> bool {
    result.is_error
        && matches!(
            result.content.as_slice(),
            [ToolResultContent::Text(text)] if text.text == NO_RESULT_PROVIDED
        )
}

// endregion: --- Support
