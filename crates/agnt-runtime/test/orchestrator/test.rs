//! Interactive-loop orchestrator test suite.
//!
//! The suite is split into topical sub-files. Shared imports, the `TResult`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

mod a2a_cancel;
mod context_window;
mod loop_lifecycle;
mod loop_lifecycle_2;
mod loop_lifecycle_3;
mod loop_phases;
mod loop_teardown;
mod mcp_toggle;
mod model_switch;
mod permission_bridge;
mod permission_flow;
mod slash_commands;

// endregion: --- Modules

use agnt_kernel::CompactionSource;

use super::test_shared::*;
use crate::orchestrator::events::UiRequest;
use crate::protocol::event::PermissionDecisionSubmission;
use crate::protocol::events::{RequestId, ToolCall};
use agnt_kernel::ToolName;

type TResult = core::result::Result<(), Box<dyn std::error::Error>>;
