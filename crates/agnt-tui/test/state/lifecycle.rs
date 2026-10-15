//! Unit tests for the app lifecycle: prompt queue, token accounting, input
//! history, and transcript bookkeeping.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "lifecycle/cancel_restore.rs"]
mod cancel_restore;

#[path = "lifecycle/history.rs"]
mod history;

#[path = "lifecycle/misc.rs"]
mod misc;

#[path = "lifecycle/queue.rs"]
mod queue;

#[path = "lifecycle/support.rs"]
mod support;

#[path = "lifecycle/tokens.rs"]
mod tokens;

// The monolith resolved `super::<item>` against `crate::state`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
