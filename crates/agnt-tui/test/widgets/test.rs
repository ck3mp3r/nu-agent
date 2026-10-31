//! TUI backend and widget test suite.
//!
//! Every test renders one entry through `RatatuiBackend` into its owned buffer
//! and asserts cells: the widgets are testable without the render loop. The
//! suite is split into topical files; shared fixtures live in `support`.

// region:    --- Modules

#[path = "banner.rs"]
mod banner;

#[path = "backend.rs"]
mod backend;

#[path = "message.rs"]
mod message;

#[path = "notice.rs"]
mod notice;

#[path = "support.rs"]
mod support;

#[path = "tool.rs"]
mod tool;

// endregion: --- Modules
