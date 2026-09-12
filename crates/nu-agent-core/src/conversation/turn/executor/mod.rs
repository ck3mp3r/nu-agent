//! Owns the logic for executing a single agent turn: building the rig agent,
//! dispatching through the hook, handling cancel paths, and persisting results.
//!
//! Extracted from `AgentConversationRuntime::execute_turn` to give it a single
//! responsibility. `AgentConversationRuntime` constructs a `TurnExecutor` and delegates.

// region:    --- Modules

mod core;
mod dispatch;
mod error_kind;
mod response;
mod retry;

pub use core::*;
pub use error_kind::*;
pub use response::*;

#[cfg(test)]
#[path = "test_utils.rs"]
pub(super) mod test_utils;

#[cfg(test)]
#[path = "executor_test_support.rs"]
mod executor_test_support;

#[cfg(test)]
#[path = "executor_hard_error_test.rs"]
mod executor_hard_error_test;

#[cfg(test)]
#[path = "executor_cancel_test.rs"]
mod executor_cancel_test;

#[cfg(test)]
#[path = "executor_repair_test.rs"]
mod executor_repair_test;

#[cfg(test)]
#[path = "executor_retry_test.rs"]
mod executor_retry_test;

#[cfg(test)]
#[path = "executor_feedback_test.rs"]
mod executor_feedback_test;

#[cfg(test)]
#[path = "executor_output_budget_test.rs"]
mod executor_output_budget_test;

#[cfg(test)]
#[path = "executor_max_turns_test.rs"]
mod executor_max_turns_test;

#[cfg(test)]
#[path = "executor_repetition_test.rs"]
mod executor_repetition_test;

#[cfg(test)]
#[path = "executor_construction_test.rs"]
mod executor_construction_test;

#[cfg(test)]
#[path = "executor_memory_append_test.rs"]
mod executor_memory_append_test;

#[cfg(test)]
#[path = "executor_log_preview_test.rs"]
mod executor_log_preview_test;

#[cfg(test)]
#[path = "journey_test.rs"]
mod journey_test;

// endregion: --- Modules
