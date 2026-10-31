//! Shared orchestrator test fixtures.
//!
//! The suite is split into topical sub-files. Shared imports and the fixture
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

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

pub(crate) use crate::bus::{ExternalEvent, create_bus};
pub(crate) use crate::orchestrator::{
    InteractiveLoopConfig, OrchestratorEvent, UiRequest, UiStateEvent, run_interactive_loop_impl,
    run_single_turn,
};
pub(crate) use crate::protocol::{
    contracts::{
        CoreRuntime, McpToggleRequest, McpUsabilityState, SharedUiAction, UiMessageSnapshot,
        UiMessageUsageSnapshot, UserInputUi,
    },
    event::{PermissionDecision, ToolDisplay, ToolDisplaySection},
    events::SystemEvent,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
    slash::{SlashParseResult, parse_slash_command},
};
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
pub(crate) use fake_interactive_ui::*;
pub(crate) use fake_runtime::*;
pub(crate) use long_running_runtime::*;
pub(crate) use responsive_interactive_ui::*;
