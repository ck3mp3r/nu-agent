//! Markdown table projection test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "clamp.rs"]
mod clamp;

#[path = "floor.rs"]
mod floor;

// endregion: --- Modules

// Sub-files resolve `super::` against this module root, so the shared fixtures
// re-exported by the markdown test root are re-exported again here.
use super::*;
