//! Unit tests for the CLI execution modes.
//!
//! The suite is split into topical sub-files. Shared imports and the `Result`
//! alias live here, the test doubles live in `support.rs`, and each sub-file
//! pulls both in with `use super::*;`.

// region:    --- Modules

#[path = "a2a.rs"]
mod a2a;

#[path = "lifecycle.rs"]
mod lifecycle;

#[path = "single_turn.rs"]
mod single_turn;

#[path = "spawn.rs"]
mod spawn;

#[path = "support.rs"]
mod support;

// endregion: --- Modules

use super::*;

use agnt_a2a::{InMemoryTaskStore, Task, TaskState};
use agnt_core::bus::{AgentEvent, AgentTx};
use agnt_core::config::{HookEvent, ResolvedHook};
use nu_protocol::Span;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
