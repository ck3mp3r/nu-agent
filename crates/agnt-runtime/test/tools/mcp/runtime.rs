//! MCP runtime test suite.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "runtime/bearer_vault.rs"]
mod bearer_vault;
#[path = "runtime/connect.rs"]
mod connect;
#[path = "runtime/exposed_names.rs"]
mod exposed_names;
#[path = "runtime/http_auth.rs"]
mod http_auth;
#[path = "runtime/server_state.rs"]
mod server_state;
#[path = "runtime/stdio.rs"]
mod stdio;
#[path = "runtime/support.rs"]
mod support;

// endregion: --- Modules

use std::collections::HashMap;

use http::HeaderName;

use crate::config::vault::Vault;
use crate::tools::mcp::{
    client::McpToolDefinition,
    config::{McpAuthConfig, McpServerConfig, McpTransportType},
};

use super::{
    McpRuntime, build_http_transport_config, compose_exposed_tool_name, merged_stdio_env_with_pwd,
    register_exposed_name, resolve_caller_cwd, resolve_stdio_cwd, select_enabled_servers,
    validate_raw_tool_name,
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// The monolith resolved `super::<helper>` against `crate::tools::mcp::runtime`.
// Sub-files now resolve `super::` against this module root, so the shared
// fixtures are re-exported here.
use support::*;
