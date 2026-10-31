//! Reducer tests for the system-event fold.
//! Domain-local effect assertions live in `state/{tool,llm,compaction,turn}_test.rs`;
//! the tests here cover orchestration-level behavior (phase, input lock,
//! abort, finalize boundaries) through `reduce`.
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

use crate::test_support::entry_text;
use crate::{
    interaction::reducer::{
        ESC_ABORT_CONFIRM_STATUS, VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS, reduce,
    },
    state::{AppState, InputMode, InputState, PaneFocus, PromptStatus, ScrollState, UiPhase},
};
use agnt_core::protocol::event::{PermissionRequestContext, ToolDisplay, ToolDisplaySection};
use agnt_core::protocol::events::{Key, SystemEvent};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_core::transcript::ir::MessageRole;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
