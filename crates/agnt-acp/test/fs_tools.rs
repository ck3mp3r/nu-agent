//! Unit tests for the ACP filesystem tools.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "fs_tools/edit.rs"]
mod edit;

#[path = "fs_tools/read.rs"]
mod read;

#[path = "fs_tools/support.rs"]
mod support;

// The original monolith declared the fixtures at its own module root.
// Sub-files resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
