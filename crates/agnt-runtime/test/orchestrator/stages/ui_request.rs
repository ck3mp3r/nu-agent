//! Unit tests for the ui_request stage.
//!
//! The suite is split into topical sub-files. Shared imports and the harness
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "ui_request/blocking.rs"]
mod blocking;

#[path = "ui_request/concurrent.rs"]
mod concurrent;

#[path = "ui_request/dispatch.rs"]
mod dispatch;

#[path = "ui_request/queue.rs"]
mod queue;

#[path = "ui_request/support.rs"]
mod support;

// The monolith resolved `super::<item>` against `crate::orchestrator::stages`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
