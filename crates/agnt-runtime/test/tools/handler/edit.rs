//! Edit tool test suite.
//!
//! The suite is split into topical sub-files — `apply`, `contract_errors` and
//! `preview`, declared below. The items they share are declared in this module,
//! and each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "edit/apply.rs"]
mod apply;
#[path = "edit/contract_errors.rs"]
mod contract_errors;
#[path = "edit/preview.rs"]
mod preview;

// endregion: --- Modules

use super::*;
use crate::tools::handler::builtin_tool::{BuiltinTool, Previewable};
use agnt_kernel::bus::CancelTx;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// A sub-file's `super::` is this module root, so the imports and the `Result`
// alias the three suites share are declared here.
