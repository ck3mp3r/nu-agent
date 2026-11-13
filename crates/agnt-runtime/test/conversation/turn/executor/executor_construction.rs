//! TurnExecutor construction, compaction, and token-accounting tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_meter, default_output_repetition,
    default_repetition_guard,
};
use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_compaction_config, test_config};
use super::*;
use crate::conversation::state::memory::MemoryState;

// region:    --- Modules

#[path = "executor_construction/compaction_ladder.rs"]
mod compaction_ladder;

#[path = "executor_construction/completed_turn.rs"]
mod completed_turn;

#[path = "executor_construction/construction.rs"]
mod construction;

#[path = "executor_construction/support.rs"]
mod support;

#[path = "executor_construction/token_accounting.rs"]
mod token_accounting;

// endregion: --- Modules
