//! Unit tests for the orchestrator stages.
//!
//! The suite is split into topical sub-files. Shared imports and the harness
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "stage/agent_picker.rs"]
mod agent_picker;

#[path = "stage/concurrent.rs"]
mod concurrent;

#[path = "stage/external_turns.rs"]
mod external_turns;

#[path = "stage/session_picker.rs"]
mod session_picker;

#[path = "stage/session_switch.rs"]
mod session_switch;

#[path = "stage/support.rs"]
mod support;

#[path = "stage/switch_warnings.rs"]
mod switch_warnings;

#[path = "stage/worker_results.rs"]
mod worker_results;

// The monolith resolved `super::<item>` against `crate::orchestrator`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
