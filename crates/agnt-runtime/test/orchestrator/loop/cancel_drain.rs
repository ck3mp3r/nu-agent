//! The user-cancel drain of the loop's queued external prompts and buffered
//! completions.
//!
//! Every scenario drives `run_orchestrator_loop` over the control and
//! completion intakes with mock stages, so the drains the cancel arm applies
//! are observed on what the stages never saw. The suite is split into topical
//! sub-files; shared imports live here and each sub-file pulls them in with
//! `use super::*;`.

// region:    --- Modules

#[path = "cancel_drain/completions.rs"]
mod completions;
#[path = "cancel_drain/prompts.rs"]
mod prompts;

// endregion: --- Modules

use super::*;
