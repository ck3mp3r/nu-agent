//! Runtime picker test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "pickers/agents/rows.rs"]
mod agents_rows;

#[path = "pickers/agents/footer.rs"]
mod agents_footer;

#[path = "pickers/models.rs"]
mod models;

#[path = "pickers/popup_bounds.rs"]
mod popup_bounds;

#[path = "pickers/sessions.rs"]
mod sessions;

#[path = "pickers/support.rs"]
mod support;

#[path = "pickers/themes.rs"]
mod themes;

// The monolith resolved `super::render::frame::*` and the picker cell
// helpers against `crate::runtime::test`. Sub-files now resolve `super::`
// against this module root, so `use support::*;` re-exports those names here.
use support::*;

// endregion: --- Modules
