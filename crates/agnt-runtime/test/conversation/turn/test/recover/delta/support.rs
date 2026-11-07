//! Shared fixtures for the `persist_turn_delta` test modules.
//!
//! Holds the recovery/record builders and the warning collector every topical
//! `delta` test file needs. Sub-files pull them in with `use super::*;`.

use super::*;

use crate::protocol::events::SystemKind;

/// The warning `persist_turn_delta` publishes when the store moved mid-turn.
pub(super) const MOVED_MID_TURN_WARNING: &str =
    "Turn stopped: the session log moved mid-turn; this turn's work was not saved";

/// The texts of every warning `SystemMessage` the executor published.
pub(super) fn warnings(rx: &mut crate::bus::UiEventRx) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|event| match event {
            SystemEvent::SystemMessage {
                kind: SystemKind::Warning,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

/// The recovery a stopped turn reports: its tool records and its streamed text.
pub(super) fn recovery<'a>(
    records: &'a [TurnToolRecord],
    text: Option<&'a str>,
) -> TurnRecovery<'a> {
    TurnRecovery {
        tool_records: records,
        streamed_text: text,
    }
}

/// A tool record for `call_id` with the settled result `result`.
pub(super) fn record(call_id: &str, result: Option<&str>) -> Result<TurnToolRecord> {
    Ok(TurnToolRecord {
        call_id: CallId::from_wire(call_id),
        name: ToolName::new("read_file")?,
        arguments: "{}".to_string(),
        result: result.map(str::to_string),
        success: result.map(|_| true),
        error_kind: None,
        status: result.map(|_| ToolStatus::Success),
    })
}

/// The result texts of the tool results in `message`, in order.
pub(super) fn result_texts(message: &Message) -> Vec<String> {
    let Message::User { content } = message else {
        return vec![];
    };
    content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => result
                .content
                .first()
                .and_then(|part| part.as_text())
                .map(str::to_string),
            _ => None,
        })
        .collect()
}
