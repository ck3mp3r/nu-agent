//! Recovery-persistence tests for a turn that did not complete.
//!
//! `TurnExecutor::persist_turn_delta` appends the interrupted turn's delta to
//! the session's rig log, anchored on the turn's prompt. These tests drive it
//! directly over an in-memory rig store.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "recover/close.rs"]
mod close;

#[path = "recover/delta.rs"]
mod delta;

#[path = "recover/grace.rs"]
mod grace;

#[path = "recover/retry.rs"]
mod retry;

#[path = "recover/support.rs"]
mod support;

use support::*;

// endregion: --- Modules

use super::*;

use crate::conversation::state::memory::MemoryState;
use crate::conversation::turn::executor::{ExecuteInput, TurnExecutor, TurnOutcome};
use crate::session::RigStoreConfig;
use crate::types::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolResult,
    ToolResultContent, UserContent,
};

/// The placeholder reply an interrupted turn persists when it recovered nothing.
const INTERRUPTED_REPLY: &str = "[interrupted — turn stopped before responding]";
