//! Edit tool test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "edit/apply.rs"]
mod apply;
#[path = "edit/call_line.rs"]
mod call_line;
#[path = "edit/contract_errors.rs"]
mod contract_errors;
#[path = "edit/preview.rs"]
mod preview;
#[path = "edit/support.rs"]
mod support;

// endregion: --- Modules

use super::*;
use crate::bus::Bus;
use crate::protocol::tool_args::CallLine;
use crate::tools::handler::builtin_tool::{BuiltinTool, Previewable};
use crate::transcript::ir::ContentKind;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The monolith resolved `super::<helper>` against `crate::tools::handler::edit`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
