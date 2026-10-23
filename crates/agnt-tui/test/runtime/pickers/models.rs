//! Model picker test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "models/cells.rs"]
mod cells;

#[path = "models/columns.rs"]
mod columns;

#[path = "models/structure/dim.rs"]
mod dim;

#[path = "models/structure/headers.rs"]
mod headers;

#[path = "models/structure/support.rs"]
mod support;

// The monolith resolved `super::render::frame::*` and the picker cell
// helpers against the pickers hub. Sub-files now resolve `super::` against
// this module root, so `use super::*;` re-exports those names here.
use super::*;

// endregion: --- Modules
