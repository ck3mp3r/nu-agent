//! The MCP runtime: the server lifecycle the surface shows, the transport arms
//! that connect servers, and the stdio/HTTP helpers those arms share.

use crate::tools::mcp::{client::McpToolDefinition, namespaced::NamespacedClientHandler};

// region:    --- Modules

mod connect;
mod http;
mod servers;
mod state;
mod stdio;

pub use connect::connect_servers;

#[cfg(test)]
pub(crate) use http::build_http_transport_config;
#[cfg(test)]
pub(crate) use servers::{
    compose_exposed_tool_name, register_exposed_name, select_enabled_servers,
    validate_raw_tool_name,
};
#[cfg(test)]
pub(crate) use stdio::{merged_stdio_env_with_pwd, resolve_caller_cwd, resolve_stdio_cwd};

// endregion: --- Modules

pub struct McpRuntime {
    sessions: Vec<McpSessionHandle>,
    connected_servers: std::collections::BTreeSet<String>,
    discovered_tools: Vec<McpToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerLifecycle {
    pub name: String,
    pub configured: bool,
    pub enabled: bool,
    pub connected: bool,
    pub visible_tool_count: usize,
}

pub(super) enum McpSessionHandle {
    Rmcp {
        _service: rmcp::service::RunningService<rmcp::service::RoleClient, NamespacedClientHandler>,
    },
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../../test/tools/mcp/runtime.rs"]
mod runtime_test;

// endregion: --- Tests
