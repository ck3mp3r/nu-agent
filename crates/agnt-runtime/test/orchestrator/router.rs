//! Tests for CommandRouter agent-switch dispatch through the interactive loop.
//!
//! These verify that SwitchAgent commands dispatched by CommandRouter on the
//! worker thread produce the correct UI updates when the orchestrator loop
//! processes the response channel.
//!
//! The suite is split into topical sub-files. Shared imports and the mock
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "router/agent_switch.rs"]
mod agent_switch;

#[path = "router/attach_context.rs"]
mod attach_context;

#[path = "router/dispatch.rs"]
mod dispatch;

#[path = "router/session_switch.rs"]
mod session_switch;

#[path = "router/support.rs"]
mod support;

// The monolith resolved `super::<item>` against `crate::orchestrator`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules
