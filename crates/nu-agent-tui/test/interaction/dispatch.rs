//! Terminal-event dispatch test suite.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

mod abort_cancel;
mod agent_picker;
mod command_palette;
mod dispatch_support;
mod info_panels;
mod inline_slash;
mod mcp_panel;
mod mode_switching;
mod model_picker;
mod models_launcher;
mod permission_prompt;
mod submit_flow;
mod visual_mode;

use dispatch_support::*;

// endregion: --- Modules

use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey},
        reducer::ESC_ABORT_CONFIRM_STATUS,
    },
    state::{
        ActivePicker, AppState, InfoPanel, InputMode, InputState, McpServerState,
        McpServerUsabilityState, PickerRenderKind, SwitchRequest, UiPhase,
    },
};
use nu_agent_core::protocol::contracts::SharedUiAction;
use nu_agent_core::protocol::event::PermissionDecision;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
