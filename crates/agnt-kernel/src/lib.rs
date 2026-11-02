//! agnt-kernel — the eventing substrate of nu-agent.
//!
//! Invariant 0 (ADR 64cbfced): the kernel contains data and eventing
//! mechanics only. No surface may appear in this crate — tui, a2a, acp,
//! plugin — vocabulary and naming included.

// region:    --- Modules

pub mod bus;
pub mod records;

pub use records::{InferenceRecord, SessionRecord, TurnRecord, Usage};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/bus/test.rs"]
mod bus_test;

#[cfg(test)]
#[path = "../test/records.rs"]
mod records_test;

// endregion: --- Tests
