//! agnt-kernel — the eventing substrate of nu-agent.
//!
//! Invariant 0 (ADR 64cbfced): the kernel contains data and eventing
//! mechanics only. No surface may appear in this crate — tui, a2a, acp,
//! plugin — vocabulary and naming included.

// region:    --- Modules

pub mod builtin;
pub mod bus;
pub mod records;
pub mod vocabulary;

pub use builtin::{BuiltinKind, ToolKind};
pub use vocabulary::ToolName;

pub use records::{
    AutoResolution, CompactionRecord, CompactionSource, InferenceRecord, NoticeKind, NoticeRecord,
    PermissionDecision, PermissionRecord, RequestId, SessionId, SessionLifecycle, SessionRecord,
    StopCause, ToolCall, ToolCallId, ToolErrorKind, ToolOutcome, ToolRecord, ToolSource,
    ToolStatus, TurnOutcome, TurnRecord, Usage,
};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "../test/builtin.rs"]
mod builtin_test;

#[cfg(test)]
#[path = "../test/bus/test.rs"]
mod bus_test;

#[cfg(test)]
#[path = "../test/records/inference.rs"]
mod records_inference_test;

#[cfg(test)]
#[path = "../test/records/tool.rs"]
mod records_tool_test;

#[cfg(test)]
#[path = "../test/records/permission.rs"]
mod records_permission_test;

#[cfg(test)]
#[path = "../test/records/compaction.rs"]
mod records_compaction_test;

#[cfg(test)]
#[path = "../test/records/session.rs"]
mod records_session_test;

// endregion: --- Tests
