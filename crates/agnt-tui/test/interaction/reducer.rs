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

use crate::interaction::input::{Key, UiInput};
use crate::test_support::{
    entry_text, inference_chunk, inference_completed, inference_started, system_message,
    tool_completed, tool_started, turn_completed,
};
use crate::{
    interaction::reducer::{
        ESC_ABORT_CONFIRM_STATUS, RenderInput, VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS, reduce,
    },
    state::{AppState, InputMode, InputState, PaneFocus, PromptStatus, ScrollState, UiPhase},
};
use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::MessageRole;
use agnt_runtime::protocol::events::{PermissionRecord, SessionRecord};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
