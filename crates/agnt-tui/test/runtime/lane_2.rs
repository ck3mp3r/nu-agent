//! Lane 2 (context usage) test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "lane_2/context_line.rs"]
mod context_line;

#[path = "lane_2/hydration.rs"]
mod hydration;

#[path = "lane_2/layout_alignment.rs"]
mod layout_alignment;

#[path = "lane_2/rehydration.rs"]
mod rehydration;

// The monolith resolved `super::<item>` against `crate::runtime`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use super::*;

// endregion: --- Modules
