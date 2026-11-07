//! Shared orchestrator test fixtures.
//!
//! The suite is split into topical sub-files. Shared imports and the fixture
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "shared/drive_loop.rs"]
mod drive_loop;
#[path = "shared/fake_interactive_ui.rs"]
mod fake_interactive_ui;
#[path = "shared/fake_runtime.rs"]
mod fake_runtime;
#[path = "shared/long_running_runtime.rs"]
mod long_running_runtime;
#[path = "shared/responsive_interactive_ui.rs"]
mod responsive_interactive_ui;

// endregion: --- Modules

pub(crate) use nu_protocol::{LabeledError, Span, Value};
pub(crate) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub(crate) use std::time::Duration;

pub(crate) use crate::orchestrator::{Loop, LoopConfig, run_single_turn};
pub(crate) use crate::session::management::{SessionPersistence, SessionState};
pub(crate) use agnt_runtime::bus::{Bus, ExternalEvent};
pub(crate) use agnt_runtime::orchestrator::events::{OrchestratorEvent, UiRequest, UiStateEvent};
pub(crate) use agnt_runtime::protocol::McpManagement;
pub(crate) use agnt_runtime::protocol::contracts::{
    CoreRuntime, McpToggleRequest, McpUsabilityState, SharedUiAction, UiMessageSnapshot,
    UiMessageUsageSnapshot, UserInputUi,
};
pub(crate) use agnt_runtime::protocol::event::PermissionDecision;
pub(crate) use agnt_runtime::protocol::events::SystemEvent;
pub(crate) use agnt_runtime::protocol::model_switching::ModelSwitching;
pub(crate) use agnt_runtime::protocol::slash::{SlashParseResult, parse_slash_command};
pub(crate) use tokio::sync::mpsc;

// Convenience: empty impl blocks that use the trait's default methods.
// Since the protocol traits now provide defaults, test runtimes that don't
// need custom behavior only need these empty blocks to satisfy trait bounds.
#[macro_export]
macro_rules! default_session {
    ($t:ty) => {
        impl SessionState for $t {}
        impl SessionPersistence for $t {}
    };
}
#[macro_export]
macro_rules! default_mcp {
    ($t:ty) => {
        impl McpManagement for $t {}
    };
}

pub(crate) type McpDetail = (String, McpUsabilityState, Option<String>, usize);

// The monolith resolved `super::<helper>` against `crate::orchestrator`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
pub(crate) use drive_loop::*;
pub(crate) use fake_interactive_ui::*;
pub(crate) use fake_runtime::*;
pub(crate) use long_running_runtime::*;
pub(crate) use responsive_interactive_ui::*;
