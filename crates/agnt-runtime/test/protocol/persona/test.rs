//! Persona protocol test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/builtins.rs"]
mod builtins;

#[path = "test/catalog.rs"]
mod catalog;

#[path = "test/front_matter.rs"]
mod front_matter;

#[path = "test/icon.rs"]
mod icon;

#[path = "test/interpret.rs"]
mod interpret;

#[path = "test/resolution.rs"]
mod resolution;

#[path = "test/support.rs"]
mod support;

// The monolith resolved `super::<helper>` against `crate::protocol::persona`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
