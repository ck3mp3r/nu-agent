//! agnt-kernel — the eventing substrate of nu-agent.
//!
//! Invariant 0 (ADR 64cbfced): the kernel contains data and eventing
//! mechanics only. No surface may appear in this crate — tui, a2a, acp,
//! plugin — vocabulary and naming included.

// region:    --- Modules

pub mod bus;
pub mod records;

pub use records::{
    AutoResolution, InferenceRecord, NoticeKind, NoticeRecord, PermissionDecision,
    PermissionRecord, RequestId, SessionRecord, StopCause, ToolCall, ToolCallId, ToolOutcome,
    ToolRecord, ToolStatus, TurnRecord, Usage,
};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;

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

// endregion: --- Tests
