//! Transcript IR test suite: ContentKind, Display, and DisplaySection.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

use super::ir::*;

// region:    --- Modules

#[path = "ir/annotate.rs"]
mod annotate;
#[path = "ir/content.rs"]
mod content;
#[path = "ir/display.rs"]
mod display;

// endregion: --- Modules
