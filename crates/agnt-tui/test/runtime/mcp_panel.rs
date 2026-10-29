//! MCP panel test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "mcp_panel/controls_line.rs"]
mod controls_line;

#[path = "mcp_panel/details_bottom_pin.rs"]
mod details_bottom_pin;

#[path = "mcp_panel/details_layout.rs"]
mod details_layout;

#[path = "mcp_panel/frame_repaint.rs"]
mod frame_repaint;

#[path = "mcp_panel/selected_details.rs"]
mod selected_details;

#[path = "mcp_panel/table_contract.rs"]
mod table_contract;

#[path = "mcp_panel/table_ellipsis.rs"]
mod table_ellipsis;

#[path = "mcp_panel/table_model.rs"]
mod table_model;

#[path = "mcp_panel/title_ellipsis.rs"]
mod title_ellipsis;

// The monolith resolved `super::<item>` against `crate::runtime`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use super::*;

// endregion: --- Modules
