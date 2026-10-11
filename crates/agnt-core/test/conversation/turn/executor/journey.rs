//! Journey integration tests: multi-turn, tool use, persistence, and cancellation.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "journey/basic.rs"]
mod basic;
#[path = "journey/errors.rs"]
mod errors;
#[path = "journey/repair.rs"]
mod repair;
#[path = "journey/repetition.rs"]
mod repetition;
#[path = "journey/retry.rs"]
mod retry;
#[path = "journey/support.rs"]
mod support;
#[path = "journey/trace.rs"]
mod trace;
#[path = "journey/truncation.rs"]
mod truncation;
#[path = "journey/wiremock.rs"]
mod wiremock_tests;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::conversation::turn::executor`.
// Sub-files now resolve `super::` against this module root, so the shared
// journey fixtures are re-exported here.
use support::*;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::REPETITION_STEERING_NOTICE;
use super::test_utils::{MockResolver, message_text, test_config};
use super::*;
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::{
    OUTPUT_REPETITION_BACKOFF_MESSAGE, OUTPUT_REPETITION_MESSAGE, OUTPUT_REPETITION_STOP_PREFIX,
    OUTPUT_REPETITION_THRESHOLD,
};
use crate::protocol::event::UiEvent;
use crate::session::StoreEntry;
use crate::types::Message;
use crate::utils::value_ext::extract_response_text_from_value;
