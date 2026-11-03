//! Edit tool test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "edit/apply.rs"]
mod apply;
#[path = "edit/contract_errors.rs"]
mod contract_errors;
#[path = "edit/preview.rs"]
mod preview;

// endregion: --- Modules

use super::*;
use crate::bus::Bus;
use crate::tools::handler::builtin_tool::{BuiltinTool, Previewable};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The monolith resolved `super::<helper>` against `crate::tools::handler::edit`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
