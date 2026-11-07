//! The typed services the host records its session facts through.
//!
//! Each service is one concern, and each call submits one work unit: the unit
//! runs on the engine, in one transaction, so a logical write lands whole or
//! not at all. Every value is supplied by the caller — nothing here reads a
//! clock, derives an ordinal, or invents a fact.

// region:    --- Modules

mod entries;
mod settle;
mod tool_calls;
mod turns;

pub(crate) use entries::ReadTranscript;
pub use entries::{Entries, EntryTurn, MessageKey};
pub use settle::{Settle, SettleRecord};
pub use tool_calls::{CallFact, ToolCallStatus, ToolCalls};
pub use turns::{TurnFacts, Turns};

// endregion: --- Modules
