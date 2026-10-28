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

use crate::bus::create_bus;
use crate::compaction::CompactionParams;
use crate::config::models_cache::{ModelCost, ModelLimit, ModelSpec, ProviderSpec};
use crate::config::{AgentsConfig, ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};
use crate::session::{RigStoreConfig, StoreType};
use crate::tools::authz::PermissionsConfig;
use crate::tools::closure::{ClosureRegistry, CwdEngine};
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;

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
        compaction_params: CompactionParams::default(),
        proactive_threshold_pct: None,
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
        bus: create_bus(),
        render_registry: ToolRenderRegistry::default(),
        hook_context: crate::hooks::HookAgentContext::default(),
    }
}

/// A plugin config whose `[models.light]` role points at an `ollama` cache
/// row carrying a context limit and a price.
fn light_plugin_config() -> PluginConfig {
    let row = ModelSpec {
        id: "light-model".to_string(),
        name: "light-model".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 262_000,
            output: 8_192,
        },
        cost: Some(ModelCost {
            input: 0.1,
            output: 0.2,
            cache_read: None,
            cache_write: None,
        }),
        modalities: None,
    };
    PluginConfig {
        models: std::collections::HashMap::from([(
            "light".to_string(),
            ModelRoleConfig {
                model: "ollama/light-model".to_string(),
                ..ModelRoleConfig::default()
            },
        )]),
        models_cache: Some(crate::config::ModelsCache {
            providers: std::collections::HashMap::from([(
                "ollama".to_string(),
                ProviderSpec {
                    id: "ollama".to_string(),
                    name: "ollama".to_string(),
                    env: Vec::new(),
                    api: None,
                    models: std::collections::HashMap::from([("light-model".to_string(), row)]),
                },
            )]),
        }),
        ..PluginConfig::default()
    }
}

/// A plugin config whose `[models.light]` role points at a custom-named
/// provider block (`ollama-cloud`) that declares the `ollama` rig impl.
fn custom_named_light_plugin_config() -> PluginConfig {
    let row = ModelSpec {
        id: "light-model".to_string(),
        name: "light-model".to_string(),
        tool_call: true,
        limit: ModelLimit {
            context: 262_000,
            output: 8_192,
        },
        cost: Some(ModelCost {
            input: 0.1,
            output: 0.2,
            cache_read: None,
            cache_write: None,
        }),
        modalities: None,
    };
    PluginConfig {
        models: std::collections::HashMap::from([(
            "light".to_string(),
            ModelRoleConfig {
                model: "ollama-cloud/light-model".to_string(),
                ..ModelRoleConfig::default()
            },
        )]),
        providers: std::collections::HashMap::from([(
            "ollama-cloud".to_string(),
            ProviderConfig {
                provider: Some("ollama".to_string()),
                models: std::collections::HashMap::from([(
                    "light-model".to_string(),
                    ModelConfig::default(),
                )]),
                ..ProviderConfig::default()
            },
        )]),
        models_cache: Some(crate::config::ModelsCache {
            providers: std::collections::HashMap::from([(
                "ollama-cloud".to_string(),
                ProviderSpec {
                    id: "ollama-cloud".to_string(),
                    name: "ollama-cloud".to_string(),
                    env: Vec::new(),
                    api: None,
                    models: std::collections::HashMap::from([("light-model".to_string(), row)]),
                },
            )]),
        }),
        ..PluginConfig::default()
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
