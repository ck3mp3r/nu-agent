//! Shared fixtures for the journey integration test modules.
//!
//! Provides `JourneyHarness`, the mock tool infrastructure, and the message
//! assertion helpers used by every journey scenario file.
//!
//! The suite is split into topical sub-files. The sub-files are re-exported at
//! this root so `journey.rs`'s `use support::*;` keeps resolving every fixture.

// region:    --- Modules

#[path = "support/assertions.rs"]
mod assertions;

#[path = "support/harness.rs"]
mod harness;

#[path = "support/tools.rs"]
mod tools;

#[path = "support/trace.rs"]
mod trace;

// endregion: --- Modules

pub(super) use assertions::*;
pub(super) use harness::*;
pub(super) use tools::*;
pub(super) use trace::*;
