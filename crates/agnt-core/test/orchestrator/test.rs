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
mod mcp_toggle;
mod model_switch;
mod permission_bridge;
mod permission_flow;
mod slash_commands;

// endregion: --- Modules

use super::test_shared::*;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::{PermissionDecisionSubmission, UiEvent};
use crate::protocol::tool_args::CallLine;
use crate::transcript::ir::ContentKind;

type TResult = core::result::Result<(), Box<dyn std::error::Error>>;
