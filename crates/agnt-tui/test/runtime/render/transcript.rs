//! Unit tests for the transcript render pane.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "transcript/bg_tint.rs"]
mod bg_tint;

#[path = "transcript/layout.rs"]
mod layout;

#[path = "transcript/padding.rs"]
mod padding;

#[path = "transcript/preview.rs"]
mod preview;

#[path = "transcript/render_loop.rs"]
mod render_loop;

#[path = "transcript/separators.rs"]
mod separators;

#[path = "transcript/support.rs"]
mod support;

#[path = "transcript/yank.rs"]
mod yank;

// The monolith resolved `super::<item>` against `crate::runtime::render`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
