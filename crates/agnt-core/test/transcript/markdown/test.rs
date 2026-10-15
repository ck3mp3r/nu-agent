//! Markdown projection test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/diff.rs"]
mod diff;

#[path = "test/math.rs"]
mod math;

#[path = "test/nu_highlight.rs"]
mod nu_highlight;

#[path = "test/projection.rs"]
mod projection;

#[path = "test/render_lines.rs"]
mod render_lines;

#[path = "test/support.rs"]
mod support;

#[path = "test/tables.rs"]
mod tables;

#[path = "test/unwrap_fenced.rs"]
mod unwrap_fenced;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::transcript::markdown`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
