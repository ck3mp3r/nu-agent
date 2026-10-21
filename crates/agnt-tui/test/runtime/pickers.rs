//! Runtime picker test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "pickers/agents.rs"]
mod agents;

#[path = "pickers/models.rs"]
mod models;

#[path = "pickers/support.rs"]
mod support;

// The monolith resolved `super::render::frame::*` and the picker cell
// helpers against `crate::runtime::test`. Sub-files now resolve `super::`
// against this module root, so `use support::*;` re-exports those names here.
use support::*;

// endregion: --- Modules
