//! Status bar test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "status_bar/indicator_prefix.rs"]
mod indicator_prefix;

#[path = "status_bar/lane_1_contract.rs"]
mod lane_1_contract;

#[path = "status_bar/panel_layout.rs"]
mod panel_layout;

#[path = "status_bar/status_contract.rs"]
mod status_contract;

#[path = "status_bar/status_line_content.rs"]
mod status_line_content;

// The monolith resolved `super::<item>` against `crate::runtime`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use super::*;

// endregion: --- Modules
