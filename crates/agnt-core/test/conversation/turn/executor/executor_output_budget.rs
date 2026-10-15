//! OutputBudget tests: error surfacing and auto-raise on retry.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, test_config};
use super::*;
use crate::config::Config;

// region:    --- Modules

#[path = "executor_output_budget/error_surfacing.rs"]
mod error_surfacing;

#[path = "executor_output_budget/feedback_retry.rs"]
mod feedback_retry;

#[path = "executor_output_budget/raise.rs"]
mod raise;

// endregion: --- Modules
