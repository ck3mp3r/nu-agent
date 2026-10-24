//! Turn tool log concern.
//!
//! Records every tool call a turn dispatches and the result each call settles
//! with. A stop drops the in-flight batch from rig's history, so the persist
//! path reads this log after the turn to keep the calls and their settled
//! results that the history no longer carries.

use std::sync::Mutex;

use crate::types::{CallId, ToolCall, ToolName};

// region:    --- Types

/// One tool call a turn dispatched, with the result it settled with.
///
/// `result`, `success`, and `error_kind` stay `None` until the call's outcome
/// arrives. A call the turn stopped before it settled keeps them `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnToolRecord {
    /// The call the model emitted, as the dispatch and its outcome carry it.
    pub call_id: CallId,
    /// The tool the call targets.
    pub name: ToolName,
    /// The JSON arguments the dispatch carried.
    pub arguments: String,
    /// The rendered result text, once the call settled.
    pub result: Option<String>,
    /// The structural success verdict, once the call settled.
    pub success: Option<bool>,
    /// The error kind, when the settled result carried an error.
    pub error_kind: Option<String>,
}

/// Accumulates a turn's tool records in dispatch order.
///
/// The hook records into it; the executor reads it after the turn through the
/// `Arc` it cloned before the hook moved into the agent builder.
#[derive(Debug, Default)]
pub struct TurnToolLog {
    records: Mutex<Vec<TurnToolRecord>>,
}

// endregion: --- Types

// region:    --- TurnToolLog

impl TurnToolLog {
    /// Record a dispatched call.
    ///
    /// A call already recorded keeps its record: one call id is one record,
    /// so a dispatch that fires twice for the same call cannot duplicate it.
    pub fn record_call(&self, call_id: CallId, name: ToolName, arguments: impl Into<String>) {
        let mut records = self.records.lock().expect("turn tool log mutex poisoned");
        if records.iter().any(|record| record.call_id == call_id) {
            return;
        }
        records.push(TurnToolRecord {
            call_id,
            name,
            arguments: arguments.into(),
            result: None,
            success: None,
            error_kind: None,
        });
    }

    /// Record the result of the call `call_id` answers.
    ///
    /// The result lands on the record the call's dispatch made. A result whose
    /// call was never recorded appends a record carrying the result fields, so
    /// a settled result is never dropped for want of a dispatch.
    pub fn record_result(
        &self,
        call_id: CallId,
        name: ToolName,
        arguments: impl Into<String>,
        result: impl Into<String>,
        success: bool,
        error_kind: Option<String>,
    ) {
        let mut records = self.records.lock().expect("turn tool log mutex poisoned");
        let result = result.into();
        if let Some(record) = records.iter_mut().find(|record| record.call_id == call_id) {
            record.result = Some(result);
            record.success = Some(success);
            record.error_kind = error_kind;
            return;
        }
        records.push(TurnToolRecord {
            call_id,
            name,
            arguments: arguments.into(),
            result: Some(result),
            success: Some(success),
            error_kind,
        });
    }

    /// The turn's records, in dispatch order.
    pub fn records(&self) -> Vec<TurnToolRecord> {
        self.records
            .lock()
            .expect("turn tool log mutex poisoned")
            .clone()
    }
}

// endregion: --- TurnToolLog

// region:    --- Functions

/// The streamed calls the turn never dispatched, in streamed order.
///
/// `record_call` fires at the top of `on_dispatch`, before the cancel check,
/// so every dispatched call has a record. A batch's siblings that a stop left
/// unpolled have none: the streamed call whose id matches no record is the
/// call the turn never ran.
pub(crate) fn undispatched_streamed_calls(
    streamed: &[ToolCall],
    records: &[TurnToolRecord],
) -> Vec<ToolCall> {
    streamed
        .iter()
        .filter(|call| !records.iter().any(|record| record.call_id == call.id))
        .cloned()
        .collect()
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/hook/turn_tools.rs"]
mod turn_tools_test;

// endregion: --- Tests
