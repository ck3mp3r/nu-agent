//! The structural tool-call status.
//!
//! A call's status is a variant, never a reading of its result text. The turn
//! settles it when the outcome arrives, the store writes it verbatim, and the
//! resolver matches it. Three variants cover what an outcome can be: a success,
//! a failure, and a call that produced no result (a gate denial, a turn cancel,
//! a wall clock). What differs between non-produced results is content, and
//! content is not a status.

// region:    --- Modules

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{CallId, RigToolErrorKind, ToolExecutionResult};

// endregion: --- Modules

// region:    --- Types

/// The status of one tool call — re-exported from the kernel's record
/// vocabulary: the persisted representation and the narrated one are the same
/// three dispositions, never a re-derived copy.
pub use agnt_kernel::records::ToolStatus;

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

/// The record's classification for a rig tool-error kind.
///
/// One variant per stable name, because those names are exactly what a surface
/// prints: nothing is re-classified on the way in, so the rendered label cannot
/// drift between the live row and the record. A function rather than a `From`
/// impl — neither type belongs to this crate, and the kernel cannot name rig.
pub fn error_kind_of(kind: RigToolErrorKind) -> agnt_kernel::ToolErrorKind {
    use agnt_kernel::ToolErrorKind as Record;
    match kind {
        RigToolErrorKind::InvalidArgs => Record::InvalidArgs,
        RigToolErrorKind::Timeout => Record::Timeout,
        RigToolErrorKind::Cancelled => Record::Cancelled,
        RigToolErrorKind::NotFound => Record::NotFound,
        RigToolErrorKind::PermissionDenied => Record::PermissionDenied,
        RigToolErrorKind::RateLimited => Record::RateLimited,
        RigToolErrorKind::Provider => Record::Provider,
        RigToolErrorKind::Network => Record::Network,
        RigToolErrorKind::Other => Record::Other,
    }
}

// endregion: --- Functions
