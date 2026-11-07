//! Doom loop detector test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "doom_loop/args_canonicalization.rs"]
mod args_canonicalization;

#[path = "doom_loop/escalation_ladder.rs"]
mod escalation_ladder;

#[path = "doom_loop/support.rs"]
mod support;

#[path = "doom_loop/threshold_detection.rs"]
mod threshold_detection;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::hook::doom_loop`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use super::*;
