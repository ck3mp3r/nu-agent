//! Hard-error persistence tests for the turn executor.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "executor_hard_error/persistence.rs"]
mod persistence;

#[path = "executor_hard_error/prior_history.rs"]
mod prior_history;

#[path = "executor_hard_error/tool_loop.rs"]
mod tool_loop;

// endregion: --- Modules

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::config::Config;
use crate::session::StoreEntry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
