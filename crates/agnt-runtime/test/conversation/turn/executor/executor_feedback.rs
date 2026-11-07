//! Provider-feedback retry tests (model-correctable completion errors).

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_config};
use super::*;
use crate::config::Config;

// region:    --- Modules

#[path = "executor_feedback/budget.rs"]
mod budget;
#[path = "executor_feedback/steering.rs"]
mod steering;

// endregion: --- Modules
