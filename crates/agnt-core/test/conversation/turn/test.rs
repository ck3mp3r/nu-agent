//! Integration tests for the turn module.
//!
//! Drives `execute_turn` with rig's `MockCompletionModel` over a real async
//! runtime and a tempdir-backed session store. Pure unit tests live in `unit.rs`.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "test/execute_turn.rs"]
mod execute_turn;

#[path = "test/persistence.rs"]
mod persistence;

#[path = "test/proxy_cancel.rs"]
mod proxy_cancel;

#[path = "test/rig_memory.rs"]
mod rig_persistence;

#[path = "test/support.rs"]
mod support;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::conversation::turn`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here. `default_*` stay `pub(crate)` so the executor
// test modules keep resolving `super::super::test::default_*`.
pub(crate) use support::*;

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::proxy::FilteredToolProxy;
use super::*;
use crate::config::Config;
use crate::hook::permission_resolver::PermissionDecision;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::types::{ToolDefinition, ToolName};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
