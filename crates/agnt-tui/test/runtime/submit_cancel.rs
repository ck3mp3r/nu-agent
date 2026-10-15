//! Submit and cancel flow test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "submit_cancel/cancel_paths.rs"]
mod cancel_paths;

#[path = "submit_cancel/driver_eventing.rs"]
mod driver_eventing;

#[path = "submit_cancel/submit_handoff.rs"]
mod submit_handoff;

// The monolith resolved `super::<item>` against `crate::runtime`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use super::*;

// endregion: --- Modules
