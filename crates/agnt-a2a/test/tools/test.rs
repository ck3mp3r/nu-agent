//! A2A tool handler tests.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/agent.rs"]
mod agent;

#[path = "test/list.rs"]
mod list;

#[path = "test/misc.rs"]
mod misc;

#[path = "test/send.rs"]
mod send;

#[path = "test/support.rs"]
mod support;

// The monolith imported the crate root directly; sub-files now resolve
// `super::` against this module root, so the shared fixtures are re-exported
// here.
use support::*;

// endregion: --- Modules
