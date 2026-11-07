//! Doom-loop and output-repetition journey scenarios.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "repetition/cross_turn.rs"]
mod cross_turn;

#[path = "repetition/doom_loop.rs"]
mod doom_loop;

#[path = "repetition/guard_off.rs"]
mod guard_off;

#[path = "repetition/intra_stream.rs"]
mod intra_stream;

// endregion: --- Modules

use super::*;
