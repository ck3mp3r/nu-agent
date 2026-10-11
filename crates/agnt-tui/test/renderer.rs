//! TUI renderer test suite.
//!
//! The suite is split into topical sub-files. Shared fixtures live in
//! `support`; each sub-file pulls them in with `use super::support::*;`.

// region:    --- Modules

#[path = "renderer/diff_tint.rs"]
mod diff_tint;

#[path = "renderer/fill_code.rs"]
mod fill_code;

#[path = "renderer/lane_prefix.rs"]
mod lane_prefix;

#[path = "renderer/lane_styling.rs"]
mod lane_styling;

#[path = "renderer/lane_theme.rs"]
mod lane_theme;

#[path = "renderer/layout.rs"]
mod layout;

#[path = "renderer/measure.rs"]
mod measure;

#[path = "renderer/support.rs"]
mod support;

#[path = "renderer/theme_threading.rs"]
mod theme_threading;

// endregion: --- Modules
