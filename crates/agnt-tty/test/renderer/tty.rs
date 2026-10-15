//! Unit tests for the TTY renderer's `layout` function.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "tty/basics.rs"]
mod basics;

#[path = "tty/content.rs"]
mod content;

#[path = "tty/layout.rs"]
mod layout;

#[path = "tty/markdown_notice.rs"]
mod markdown_notice;

#[path = "tty/support.rs"]
mod support;

#[path = "tty/tool_display.rs"]
mod tool_display;

// The original monolith resolved `super::tty::layout` against `crate::renderer`.
// Sub-files resolve `super::` against this module root, so the shared helpers
// are re-exported here.
use support::*;

// endregion: --- Modules
