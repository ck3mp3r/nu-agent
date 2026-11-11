//! The session vocabulary, as this crate hands it to the surfaces.
//!
//! The vocabulary itself is the kernel's: one record log, one narration stream,
//! one lifecycle spine, one facts ring, one control intake. This module is the
//! import path the surfaces already use, re-exported so a surface names its
//! data without reaching across crates.

pub use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};
/// The whole record and event vocabulary a surface folds.
///
/// A drawer reads records and derives its own display from them, so it names
/// the record types, never the producer's rendering.
pub use agnt_kernel::{
    AutoResolution, CompactionRecord, CompactionSource, InferenceRecord, NoticeKind,
    PermissionRecord, PromptRecord, PromptStamps, RequestId, SessionId, SessionLifecycle,
    SessionRecord, StopCause, ToolCall, ToolCallId, ToolErrorKind, ToolName, ToolOutcome,
    ToolRecord, ToolStatus, TurnRecord, Usage,
};
