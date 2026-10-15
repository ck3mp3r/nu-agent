//! Builtin tool test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "builtin_tool/dynamic.rs"]
mod dynamic;
#[path = "builtin_tool/edit_noop.rs"]
mod edit_noop;
#[path = "builtin_tool/errors.rs"]
mod errors;
#[path = "builtin_tool/render.rs"]
mod render;
#[path = "builtin_tool/support.rs"]
mod support;

// endregion: --- Modules

use crate::types::{EmptyToolName, ToolDefinition, ToolName};
use std::path::Path;

use super::BuiltinTool;

type TestResult<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The monolith resolved `super::<helper>` against `crate::tools::handler::builtin_tool`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
