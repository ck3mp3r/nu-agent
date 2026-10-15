//! Cancel-path persistence tests for the turn executor.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::protocol::event::UiEvent;
use crate::session::StoreEntry;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::utils::value_ext::extract_response_text_from_value;

// region:    --- Modules

#[path = "executor_cancel/edge_paths.rs"]
mod edge_paths;

#[path = "executor_cancel/persistence.rs"]
mod persistence;

// endregion: --- Modules
