//! Stderr renderer contract tests: what each output mode, spinner state and
//! tool lifecycle writes, and what quiet mode suppresses.
//!
//! Split by topic: `modes.rs` holds the output modes and verbosity, `spinner.rs`
//! the spinner's gating and frames, `tool_lifecycle.rs` the tool completion
//! line, and `permission.rs` the permission cancellation line.

use super::super::StderrUiRenderer;
use super::{draw, narrate, tool_call};
use crate::renderer::UiRenderer;
use agnt_kernel::bus::NarrationEvent;
use agnt_kernel::{
    InferenceId, InferenceRecord, NoticeKind, PermissionRecord, RequestId, SessionRecord, ToolCall,
    ToolOutcome, ToolRecord, ToolStatus, TurnRecord, Usage,
};

use crate::policy::{UiPolicy, Verbosity};

#[path = "permission_auto_resolved.rs"]
mod permission_auto_resolved;

// region:    --- Modules

mod modes;
mod permission;
mod spinner;
mod tool_lifecycle;

// endregion: --- Modules
