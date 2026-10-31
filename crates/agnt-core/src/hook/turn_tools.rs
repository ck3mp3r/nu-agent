//! Turn tool log concern.
//!
//! Records every tool call a turn dispatches and the result each call settles
//! with. A stop drops the in-flight batch from rig's history, so the persist
//! path reads this log after the turn to keep the calls and their settled
//! results that the history no longer carries.

use std::sync::Mutex;

use crate::session::tool_status::{ToolStatus, ToolStatusRecord};
use crate::types::{CallId, ToolCall, ToolName};

// region:    --- Types

/// One tool call a turn dispatched, with the result it settled with.
///
/// `result`, `success`, and `error_kind` stay `None` until the call's outcome
/// arrives. A call the turn stopped before it settled keeps them `None`.
///
/// `status` is the call's structural classification, built as a variant when
/// the outcome settles. `error_kind` is the handler's own vocabulary, carried
/// for the display and the LLM-facing text: it is never read as a status.
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
    /// The structural status the outcome settled with.
    pub status: Option<ToolStatus>,
}

/// What a settled call carries: the result the dispatch produced, the
/// structural verdicts it settled with, and the handler's own error kind.
///
/// `status` is the variant built at the set point and `error_kind` is the
/// handler's vocabulary; the two never substitute for one another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettledToolCall {
    /// The rendered result text.
    pub result: String,
    /// The structural success verdict.
    pub success: bool,
    /// The structural status the call settled with.
    pub status: ToolStatus,
    /// The handler's error kind, when the result carried one.
    pub error_kind: Option<String>,
}

impl SettledToolCall {
    /// A settled call from the facts every dispatch produces.
    pub fn new(result: impl Into<String>, success: bool, status: ToolStatus) -> Self {
        Self {
            result: result.into(),
            success,
            status,
            error_kind: None,
        }
    }

    /// The same call, carrying the handler's error kind.
    pub fn with_error_kind(mut self, error_kind: Option<String>) -> Self {
        self.error_kind = error_kind;
        self
    }
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
            status: None,
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
        settled: SettledToolCall,
    ) {
        let mut records = self.records.lock().expect("turn tool log mutex poisoned");
        if let Some(record) = records.iter_mut().find(|record| record.call_id == call_id) {
            record.result = Some(settled.result);
            record.success = Some(settled.success);
            record.error_kind = settled.error_kind;
            record.status = Some(settled.status);
            return;
        }
        records.push(TurnToolRecord {
            call_id,
            name,
            arguments: arguments.into(),
            result: Some(settled.result),
            success: Some(settled.success),
            error_kind: settled.error_kind,
            status: Some(settled.status),
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

/// The status record of every call the turn dispatched.
///
/// A call the turn recorded but never settled never produced a result, so its
/// record carries the cancelled variant: the turn stopped before the call
/// completed, whether it never dispatched or was cut short mid-run.
pub(crate) fn status_records(records: &[TurnToolRecord]) -> Vec<ToolStatusRecord> {
    records
        .iter()
        .map(|record| ToolStatusRecord {
            call: record.call_id.clone(),
            status: record.status.unwrap_or(ToolStatus::Cancelled),
        })
        .collect()
}

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
