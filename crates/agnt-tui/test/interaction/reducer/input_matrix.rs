//! The input behavior matrix: one behavior per row, the standing regression net
//! for the surface's input contract — mode transitions, the lock gate, the Esc
//! arm/confirm/expiry trio, compose-under-lock, the normal scroll keys, the
//! wheel, the pickers under lock, and the stack drain.
//!
//! Every row carries its matrix number (`// row N`), so a row keeps its identity
//! wherever it lives and a task un-ignores it by number. A row whose behavior is
//! still red carries `#[ignore]` naming the task that turns it green; that task
//! removes the ignore in its own change.
//!
//! The suite is split into topical sub-files: the shared imports, the `Result`
//! alias, and the harness re-exports live here; each sub-file pulls them in with
//! `use super::*;`.

// region:    --- Modules

#[path = "input_matrix/esc.rs"]
mod esc;

#[path = "input_matrix/gate.rs"]
mod gate;

#[path = "input_matrix/lane.rs"]
mod lane;

#[path = "input_matrix/mode.rs"]
mod mode;

#[path = "input_matrix/prompt_stack.rs"]
mod prompt_stack;

#[path = "input_matrix/scroll.rs"]
mod scroll;

#[path = "input_matrix/support.rs"]
mod support;

#[path = "input_matrix/wheel.rs"]
mod wheel;

use support::*;

// endregion: --- Modules

use super::*;

use std::time::Duration;

use ratatui::layout::Rect;

use crate::interaction::cancel::CancelController;
use crate::runtime::render::bottom_box::queued_prompt_entries;
use crate::state::ActivePicker;
use agnt_runtime::protocol::events::NoticeKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
