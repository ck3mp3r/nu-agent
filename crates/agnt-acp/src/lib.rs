//! ACP (Agent Client Protocol) stdio server for nu-agent.

// region:    --- Modules

pub mod engine_stub;
pub mod error;
pub mod fs_tools;
mod mapping;
pub mod permission;
pub mod runtime;
mod serve;
pub mod session;
pub mod translator;

pub use engine_stub::*;
pub use error::*;
pub use fs_tools::*;
pub use permission::*;
pub use runtime::*;
pub use serve::serve;
pub use session::*;
pub use translator::*;

pub(crate) use mapping::{
    agent_capabilities, agent_info, handle_set_config_option, mcp_server_config, prompt_text,
    to_acp_error,
};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/error.rs"]
mod error_test;

#[cfg(test)]
#[path = "../test/permission.rs"]
mod permission_test;

// endregion: --- Tests
