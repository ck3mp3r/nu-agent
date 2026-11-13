//! Compaction orchestrator loop test suite.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compaction/dispatch.rs"]
mod dispatch;
#[path = "compaction/quit.rs"]
mod quit;
#[path = "compaction/uncompact.rs"]
mod uncompact;

// endregion: --- Modules

use super::*;
