//! Stderr renderer streaming tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compaction.rs"]
mod compaction;

#[path = "stopped.rs"]
mod stopped;

#[path = "text.rs"]
mod text;

#[path = "tool_and_usage.rs"]
mod tool_and_usage;

#[path = "verbosity.rs"]
mod verbosity;

// endregion: --- Modules

use agnt_core::protocol::event::UiEvent;
use agnt_core::renderer::UiRenderer;

use super::StderrUiRenderer;
use crate::policy::{UiPolicy, Verbosity};
