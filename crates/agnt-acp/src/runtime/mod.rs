//! Runtime construction for ACP sessions.
//!
//! The module is split by topic: `params` holds the builder's inputs,
//! `config_options` the model and agent selectors, `builder` the runtime
//! construction itself, and `support` the helpers that construction uses.

// region:    --- Modules

mod builder;
mod config_options;
mod params;
mod support;

pub use builder::build_acp_runtime;
pub use config_options::{
    AGENT_CONFIG_ID, MODEL_CONFIG_ID, build_agent_config_options, build_model_config_options,
};
pub use params::AcpRuntimeParams;

// endregion: --- Modules

// region:    --- Tests

// The suite was written against one file, so it reads the names the module's own
// imports used to put here through `super::`. They stay here, test-only, because
// no production module needs them at this depth.
#[cfg(all(test, feature = "integration"))]
use crate::error::AcpError;
#[cfg(all(test, feature = "integration"))]
use crate::session::ClientFsState;
#[cfg(test)]
use agent_client_protocol::schema::v1::SessionConfigOption;
#[cfg(test)]
use agnt_runtime::config::{AgentsConfig, Config};
#[cfg(all(test, feature = "integration"))]
use agnt_runtime::conversation::runtime::AgentConversationRuntime;
#[cfg(all(test, feature = "integration"))]
use support::mcp_tool_definition;

#[cfg(test)]
#[path = "../../test/config_options.rs"]
mod config_options_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/support.rs"]
mod runtime_support;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/construction.rs"]
mod runtime_construction;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/mcp.rs"]
mod runtime_mcp;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/client_fs.rs"]
mod runtime_client_fs;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/compaction_spec.rs"]
mod runtime_compaction_spec;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/compaction_policy.rs"]
mod runtime_compaction_policy;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/config_errors.rs"]
mod runtime_config_errors;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/agent_overlay.rs"]
mod runtime_agent_overlay;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/runtime/agent_flag.rs"]
mod runtime_agent_flag;

// endregion: --- Tests
