//! Output repetition detector test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "output_repetition/aggregated_check.rs"]
mod aggregated_check;

#[path = "output_repetition/escalation_ladder.rs"]
mod escalation_ladder;

#[path = "output_repetition/finish_stop.rs"]
mod finish_stop;

#[path = "output_repetition/segment_detection.rs"]
mod segment_detection;

#[path = "output_repetition/support.rs"]
mod support;

// endregion: --- Modules

// The monolith resolved `super::<item>` against `crate::hook::output_repetition`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use super::*;
