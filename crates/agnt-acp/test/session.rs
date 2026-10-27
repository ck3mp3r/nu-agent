//! Unit tests for the ACP session manager.
//!
//! Every test drives the public `AcpSessionManager` API against an in-memory
//! session store, so nothing touches the network or the filesystem store.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "session/agent_options.rs"]
mod agent_options;

#[path = "session/cancel.rs"]
mod cancel;

#[path = "session/config_options.rs"]
mod config_options;

#[path = "session/compaction.rs"]
mod compaction;

#[path = "session/compaction_dispatch.rs"]
mod compaction_dispatch;

#[path = "session/events.rs"]
mod events;

#[path = "session/lifecycle.rs"]
mod lifecycle;

#[path = "session/load_resume.rs"]
mod load_resume;

#[path = "session/prompt.rs"]
mod prompt;

#[path = "session/support.rs"]
mod support;

#[path = "session/switch_events.rs"]
mod switch_events;

#[path = "session/turn_outcome.rs"]
mod turn_outcome;

// The monolith resolved `super::<item>` against `crate::session`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;

// endregion: --- Modules

// `crate::session` also exports a `Result` alias; the session tests use the
// `Box<dyn Error>` alias from `support`, so the parent items are imported by
// name instead of with a glob.
use super::{
    AcpError, AcpSessionManager, ClientFsState, SessionConfigOption, TurnOutcome,
    map_turn_outcome_to_stop_reason,
};
