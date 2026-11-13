//! Integration tests for `build_runtime()`.
//!
//! `build_runtime` constructs real provider clients (an HTTP client plus TLS
//! provider initialisation), so these tests are gated behind the
//! `integration` feature.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the fixtures live here; each sub-file pulls them in with
//! `use super::*;`.

// region:    --- Modules

#[path = "compaction_model.rs"]
mod compaction_model;

#[path = "construction.rs"]
mod construction;

// endregion: --- Modules

use super::*;

use crate::bus::Bus;
use crate::config::{AgentsConfig, ModelConfig, PluginConfig, ProviderConfig, defaults};
use crate::session::management::SessionPersistence;
use crate::session::{RigStoreConfig, StoreType};
use crate::tools::authz::PermissionsConfig;
use crate::tools::closure::{ClosureRegistry, CwdEngine};
use crate::tools::handler::McpToolRegistry;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A config whose provider builds offline: `ollama` needs only a base URL.
fn offline_config() -> Config {
    Config {
        provider: "ollama".to_string(),
        model: "llama3".to_string(),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    }
}

fn make_params(
    cwd: std::path::PathBuf,
    config: Config,
    plugin_config: Option<PluginConfig>,
    store_type: StoreType,
) -> RuntimeBuildParams {
    RuntimeBuildParams {
        runtime: tokio::runtime::Handle::current(),
        config,
        plugin_config,
        tool_definitions: Vec::new(),
        baseline_tool_definitions: Vec::new(),
        closure_registry: ClosureRegistry::default(),
        mcp_runtime: None,
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        mcp_lifecycle_projection: Vec::new(),
        mcp_server_configs: Vec::new(),
        mcp_caller_cwd: None,
        vault: None,
        mcp_registry: McpToolRegistry::empty(),
        engine: CwdEngine::shared(cwd.to_string_lossy().to_string()),
        rig: RigStoreConfig::new(store_type),
        final_session_id: None,
        base_permissions: PermissionsConfig::safe_defaults(true),
        effective_permissions: PermissionsConfig::safe_defaults(true),
        cli_permissions_overlay: None,
        permissions_startup_summary: "test permissions summary".to_string(),
        persona_body: Some("persona body".to_string()),
        agent_identity: Some("test-agent".to_string()),
        agent_description: Some("test description".to_string()),
        agent_icon: Some("robot".to_string()),
        cached_agents_chain: Some("agents chain".to_string()),
        cached_available_skills: Some("available skills".to_string()),
        available_agents: Vec::new(),
        agents_config: AgentsConfig::default(),
        paths: crate::test_support::paths_in(&cwd),
        cwd,
        bus: Bus::default(),
        hook_context: crate::hooks::HookAgentContext::default(),
        compactor_model: None,
    }
}

/// A startup config whose provider is a custom config key declaring the
/// `ollama` rig impl, as `resolve_model` leaves it.
fn custom_named_startup_config() -> Config {
    Config {
        provider: "ollama-cloud".to_string(),
        model: "llama3".to_string(),
        provider_impl: Some("ollama".to_string()),
        base_url: Some("http://localhost:11434".to_string()),
        ..Config::default()
    }
}

/// The plugin config declaring the `ollama` impl for the `ollama-cloud` key.
fn custom_named_startup_plugin_config() -> PluginConfig {
    PluginConfig {
        providers: std::collections::HashMap::from([(
            "ollama-cloud".to_string(),
            ProviderConfig {
                provider: Some("ollama".to_string()),
                models: std::collections::HashMap::from([(
                    "llama3".to_string(),
                    ModelConfig::default(),
                )]),
                ..ProviderConfig::default()
            },
        )]),
        ..PluginConfig::default()
    }
}

// endregion: --- Test Support
