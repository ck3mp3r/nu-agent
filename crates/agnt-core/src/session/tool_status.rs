//! The structural tool-call status.
//!
//! A call's status is a variant, never a reading of its result text. The turn
//! settles it when the outcome arrives, the store writes it verbatim, and the
//! resolver matches it. Four variants cover what an outcome can be: a success,
//! a failure, a run the turn stopped before it produced a result, and a call
//! that never ran.
//!
//! The record lives in the session's own file layer ([`TOOL_STATUS_FILE`]),
//! beside the session `name` file. rig's seq log parses every line it holds and
//! refuses a session with a record type it does not know, so the host's
//! structural status rides the host-owned file layer instead of rig's format.
//! Both stores carry it: the directory store writes `files/tool-status.jsonl`,
//! the SQLite store a row of its `files` table.

// region:    --- Modules

use std::collections::HashMap;
use std::io;

use rig_memory::Files;
use serde::{Deserialize, Serialize};

use crate::types::{CallId, RigToolErrorKind, ToolExecutionResult};

// endregion: --- Modules

// region:    --- Constants

/// The file of a session's store that carries its tool-call statuses.
pub const TOOL_STATUS_FILE: &str = "tool-status.jsonl";

// endregion: --- Constants

// region:    --- Types

/// The status of one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// The call ran and produced a result.
    Success,
    /// The call ran and failed.
    Failed,
    /// The call never reached a result: the turn's cancel, a wall clock that
    /// killed the command, or a dispatch policy that stopped it before it ran.
    /// Every non-completed call is this one variant; the difference between them
    /// is content, and content is not a status.
    Cancelled,
}

/// One call's status, as the store carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStatusRecord {
    /// The call the status belongs to.
    pub call: CallId,
    /// The status itself.
    pub status: ToolStatus,
}

/// Every status a session's store carries, by call.
pub type ToolStatusMap = HashMap<CallId, ToolStatus>;

// endregion: --- Types

// region:    --- Functions

/// The status `result` settles with.
///
/// The structured disposition decides, never the result text: a success, a skip
/// the dispatch policy stopped (the permission gate's denial), rig's `cancelled`
/// kind (the turn's cancel or a wall clock stopped the call), and everything
/// else a failure.
pub fn status_of_result(result: &ToolExecutionResult) -> ToolStatus {
    if result.is_success() {
        return ToolStatus::Success;
    }
    if result.is_skipped() {
        return ToolStatus::Cancelled;
    }
    match result.error().map(|error| error.kind()) {
        Some(RigToolErrorKind::Cancelled) => ToolStatus::Cancelled,
        _ => ToolStatus::Failed,
    }
}

/// Append `records` to the session's status file, one JSON line each.
pub fn write_statuses(
    files: &(impl Files + ?Sized),
    records: &[ToolStatusRecord],
) -> io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let mut lines = String::new();
    for record in records {
        let line = serde_json::to_string(record).map_err(io::Error::other)?;
        lines.push_str(&line);
        lines.push('\n');
    }
    files.append_file(TOOL_STATUS_FILE, lines.as_bytes())
}

/// The statuses the session's status file carries, by call.
///
/// A session with no status file carries none: it was saved before the statuses
/// existed, and its rows read by their stored `is_error` verdict. A line nothing
/// reads is skipped with a warning, so one bad record never fails the hydration
/// of the conversation it belongs to.
pub fn read_statuses(files: &(impl Files + ?Sized)) -> ToolStatusMap {
    let bytes = match files.read_file(TOOL_STATUS_FILE) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return ToolStatusMap::new(),
        Err(error) => {
            log::warn!("failed to read the session's tool statuses: {error}");
            return ToolStatusMap::new();
        }
    };
    let mut statuses = ToolStatusMap::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        match serde_json::from_slice::<ToolStatusRecord>(line) {
            Ok(record) => {
                statuses.insert(record.call, record.status);
            }
            Err(error) => log::warn!("skipping an unreadable tool status line: {error}"),
        }
    }
    statuses
}

// endregion: --- Functions
