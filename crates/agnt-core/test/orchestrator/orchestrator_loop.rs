//! Orchestrator loop test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "loop/a2a.rs"]
mod a2a;
#[path = "loop/a2a_completion.rs"]
mod a2a_completion;
#[path = "loop/compaction.rs"]
mod compaction;
#[path = "loop/defaults.rs"]
mod defaults;
#[path = "loop/events.rs"]
mod events;
#[path = "loop/support.rs"]
mod support;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::orchestrator`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use agnt_a2a::{
    A2aCompletionEvent, InMemoryTaskStore, IncomingTask, Message, Part, Role, TaskState,
};
use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::orchestrator::stages::slash::SlashStage;
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::orchestrator::{Stages, run_orchestrator_loop};
use agnt_runtime::bus::ExternalEvent;
use agnt_runtime::orchestrator::events::{OrchestratorEvent, UiRequestResponse, WorkerCommand};
