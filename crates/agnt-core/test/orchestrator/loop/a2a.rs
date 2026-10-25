//! A2A orchestrator loop test suite.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "a2a/busy.rs"]
mod busy;
#[path = "a2a/deferred_reject.rs"]
mod deferred_reject;
#[path = "a2a/intake.rs"]
mod intake;
#[path = "a2a/saturation.rs"]
mod saturation;

// endregion: --- Modules

use super::*;
