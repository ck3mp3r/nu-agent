//! Picker state test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "picker/agent_cycle.rs"]
mod agent_cycle;

#[path = "picker/agents.rs"]
mod agents;

#[path = "picker/command_palette.rs"]
mod command_palette;

#[path = "picker/container.rs"]
mod container;

#[path = "picker/conversions.rs"]
mod conversions;

#[path = "picker/inline_slash.rs"]
mod inline_slash;

#[path = "picker/models.rs"]
mod models;

#[path = "picker/options_sorting.rs"]
mod options_sorting;

#[path = "picker/picker_state.rs"]
mod picker_state;

#[path = "picker/selection_position.rs"]
mod selection_position;

#[path = "picker/submit.rs"]
mod submit;

#[path = "picker/support.rs"]
mod support;

// The monolith resolved `super::<helper>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
