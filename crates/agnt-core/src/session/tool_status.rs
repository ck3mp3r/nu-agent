//! The structural tool-call status.
//!
//! A call's status is a variant, never a reading of its result text. The turn
//! settles it when the outcome arrives, the store writes it verbatim, and the
//! resolver matches it. Four variants cover what an outcome can be: a success,
//! a failure, a run the turn stopped before it produced a result, and a call
//! that never ran.

// region:    --- Modules

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{CallId, RigToolErrorKind, ToolExecutionResult};

// endregion: --- Modules

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

/// Every status a session recorded, by call id as the wire carries it.
///
/// Keyed by the wire form rather than by [`CallId`]: the database stores the id
/// as text, and a rig-issued (local) id reads back without the variant it was
/// made under.
pub type ToolStatusMap = HashMap<String, ToolStatus>;

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

// endregion: --- Functions
