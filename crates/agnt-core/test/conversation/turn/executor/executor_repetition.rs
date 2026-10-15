//! Doom-loop and repetition-stop tests.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::REPETITION_STEERING_NOTICE;
use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_compaction_config, test_config};
use super::*;
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::{OUTPUT_REPETITION_BACKOFF_MESSAGE, RepetitionState};
use crate::protocol::event::UiEvent;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::utils::value_ext::extract_response_text_from_value;

// region:    --- Modules

#[path = "executor_repetition/doom_loop.rs"]
mod doom_loop;

#[path = "executor_repetition/session_less.rs"]
mod session_less;

#[path = "executor_repetition/stop_ladder.rs"]
mod stop_ladder;

// endregion: --- Modules
