//! Conversation turn execution using agent hooks.
//!
//! This module provides `execute_turn` which handles a single conversation turn:
//! sending user input to the LLM, executing tool calls via hooks, and returning
//! the final response. Permission and lifecycle events flow to consumers through
//! the shared `Bus`; core never threads a `ProgressUi` through the turn.

// region:    --- Modules

mod context;
mod execute;
mod proxy;

pub use context::*;
pub use execute::*;

pub mod executor;

pub(crate) mod error;
pub use error::TurnError;

pub(crate) mod token_estimate;

// endregion: --- Modules

#[cfg(test)]
mod test;

#[cfg(test)]
#[path = "cancel_test.rs"]
mod cancel_test;
