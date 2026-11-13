//! Compaction tests for the hook chain.
//!
//! The suite is split into topical sub-files. Shared fixtures live in
//! `support`; each sub-file pulls in the chain root with `use super::super::*;`
//! and this module, fixtures included, with `use super::*;`.

// region:    --- Modules

#[path = "decide.rs"]
mod decide;

#[path = "fallback.rs"]
mod fallback;

#[path = "ladder.rs"]
mod ladder;

#[path = "metering.rs"]
mod metering;

#[path = "reserve.rs"]
mod reserve;

#[path = "store.rs"]
mod store;

#[path = "stream.rs"]
mod stream;

#[path = "support.rs"]
mod support;

#[path = "worker.rs"]
mod worker;

// endregion: --- Modules

use support::*;
