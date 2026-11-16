//! ACP session manager: the registry of live sessions of one connection.
//!
//! ACP allows several concurrent sessions per connection, so the manager owns a
//! map of session id → [`AcpSession`]. Each session owns its own
//! `AgentConversationRuntime` (tool server, bus, model) and shares the
//! process-level session store with its siblings.
//!
//! The manager is split by topic: `types` holds the session and manager types,
//! `manager` the constructors and registry accessors, `lifecycle` the
//! open/load/resume/close/delete paths, `config` the config-option setters,
//! `prompt` the turn dispatch, and `support` the free-standing helpers.

// region:    --- Modules

mod config;
mod lifecycle;
mod manager;
mod prompt;
mod support;
mod types;

pub use types::*;

// endregion: --- Modules

// region:    --- Tests

// The suite was written against one file, so it reads the names the manager's
// own imports used to put in this module through `super::`. They stay here,
// test-only, because no production module needs them at this depth.
#[cfg(test)]
use crate::error::AcpError;
#[cfg(test)]
use crate::permission::PermissionRequestSink;
#[cfg(test)]
use crate::runtime::AcpRuntimeParams;
#[cfg(test)]
use crate::translator::SessionUpdateSink;
#[cfg(test)]
use agent_client_protocol::schema::v1::SessionConfigOption;
#[cfg(test)]
use agnt_runtime::conversation::runtime::AgentConversationRuntime;
#[cfg(test)]
use support::stop_category;

#[cfg(test)]
#[path = "../../test/session.rs"]
mod session_test;

// endregion: --- Tests
