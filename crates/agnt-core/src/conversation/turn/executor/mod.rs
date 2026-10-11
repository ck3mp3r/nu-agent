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

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/utils.rs"]
pub(super) mod test_utils;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_support.rs"]
mod executor_test_support;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_hard_error.rs"]
mod executor_hard_error_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_cancel.rs"]
mod executor_cancel_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_repair.rs"]
mod executor_repair_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_retry.rs"]
mod executor_retry_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_feedback.rs"]
mod executor_feedback_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_output_budget.rs"]
mod executor_output_budget_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_max_turns.rs"]
mod executor_max_turns_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_repetition.rs"]
mod executor_repetition_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_construction.rs"]
mod executor_construction_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_memory_append.rs"]
mod executor_memory_append_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/executor_log_preview.rs"]
mod executor_log_preview_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../../../test/conversation/turn/executor/journey.rs"]
mod journey_test;

// endregion: --- Modules
