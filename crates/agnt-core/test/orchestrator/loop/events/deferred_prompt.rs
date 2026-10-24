//! Deferred prompt test suite.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "deferred_prompt/attribution.rs"]
mod attribution;
#[path = "deferred_prompt/submit.rs"]
mod submit;

// endregion: --- Modules

use super::*;
