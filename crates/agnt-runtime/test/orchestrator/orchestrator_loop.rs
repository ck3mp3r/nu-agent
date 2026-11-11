//! Orchestrator loop test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "loop/a2a.rs"]
mod a2a;
#[path = "loop/cancel_drain.rs"]
mod cancel_drain;
#[path = "loop/compaction.rs"]
mod compaction;
#[path = "loop/completions.rs"]
mod completions;
#[path = "loop/control_arms.rs"]
mod control_arms;
#[path = "loop/defaults.rs"]
mod defaults;
#[path = "loop/events.rs"]
mod events;
#[path = "external_arms.rs"]
mod external_arms;
#[path = "loop/picker_catalog_attach.rs"]
mod picker_catalog_attach;
#[path = "loop/picker_catalogs.rs"]
mod picker_catalogs;
#[path = "loop/support.rs"]
mod support;

// endregion: --- Modules

// The monolith resolved `super::<helper>` against `crate::orchestrator`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

use nu_protocol::{LabeledError, Span, Value};
use tokio::sync::mpsc;

use crate::orchestrator::events::{OrchestratorEvent, UiRequestResponse, WorkerCommand};
use crate::orchestrator::stages::slash::SlashStage;
use crate::orchestrator::turn_outcome::TurnResult;
use crate::orchestrator::{Stages, run_orchestrator_loop};
