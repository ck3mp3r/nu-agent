//! Tool state test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "tool/display.rs"]
mod display;

#[path = "tool/edit_display.rs"]
mod edit_display;

#[path = "tool/hydration.rs"]
mod hydration;

#[path = "tool/lifecycle.rs"]
mod lifecycle;

#[path = "tool/permission.rs"]
mod permission;

#[path = "tool/preview.rs"]
mod preview;

#[path = "tool/status.rs"]
mod status;

#[path = "tool/support.rs"]
mod support;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
