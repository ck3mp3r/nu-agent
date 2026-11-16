//! Unit tests for the turn module.
//!
//! Covers `TurnResult` / `TurnError` construction and conversion. Integration
//! tests that drive `execute_turn` with a mock model live in `test.rs`.
//!
//! Split by topic: `result.rs` holds construction, conversion and flag
//! propagation; `cancel.rs` holds the path-B cancel messages and the cancelled
//! history the regression tests preserve.

use super::*;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, Text, ToolCall, ToolFunction, ToolName,
    ToolResultContent, UserContent,
};
use agnt_kernel::InferenceId;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Modules

mod cancel;
mod result;

// endregion: --- Modules
