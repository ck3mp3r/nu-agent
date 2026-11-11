//! Parsing and validation of the `[[hooks]]` block.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

mod event_surface;
mod parsing;
mod resolution;
mod validation;

// endregion: --- Modules

use super::*;
use crate::config::{HookConfig, HookEvent, HookEventClass, resolve_hooks};
