//! Reducer tests for user actions and the cross-domain `UiEvent` dispatch.
//! Domain-local effect assertions live in `state/{tool,llm,compaction,turn}_test.rs`;
//! the tests here cover orchestration-level behavior (phase, input lock,
//! abort, finalize boundaries) through `reduce_with_cancel_controller`.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

#[path = "reducer/cancel.rs"]
mod cancel;

#[path = "reducer/event_matrix.rs"]
mod event_matrix;

#[path = "reducer/mode.rs"]
mod mode;

#[path = "reducer/phase.rs"]
mod phase;

#[path = "reducer/scroll.rs"]
mod scroll;

#[path = "reducer/support.rs"]
mod support;

#[path = "reducer/user_action.rs"]
mod user_action;

#[path = "reducer/visual.rs"]
mod visual;

use support::*;

// endregion: --- Modules

use crate::{
    interaction::reducer::{
        ESC_ABORT_CONFIRM_STATUS, ReducerInput, UserAction,
        VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS, dispatch_ui_event, reduce_with_cancel_controller,
    },
    state::{AppState, InputMode, InputState, PaneFocus, PromptStatus, ScrollState, UiPhase},
};
use nu_agent_core::protocol::event::{
    PermissionRequestContext, ToolDisplay, ToolDisplaySection, UiEvent,
};
use nu_agent_core::protocol::tool_args::CallLine;
use nu_agent_core::transcript::ir::{Block, BlockSource, MessageRole};
use nu_agent_core::transcript::items::Message;
use nu_agent_core::transcript::renderer::Renderable;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
