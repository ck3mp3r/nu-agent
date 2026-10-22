//! Runtime construction for ACP sessions.

// region:    --- Modules

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
};
use agnt_core::config::{AgentsConfig, Config, ModelsCache, defaults, load_and_resolve_config};
use agnt_core::conversation::builder::{build_compaction_params, builtin_tool_definitions};
use agnt_core::conversation::compaction::compactor::NuCompactor;
use agnt_core::conversation::runtime::{AgentConversationRuntime, PendingPermissions};
use agnt_core::conversation::state::{
    McpState, MemoryState, MultiAgentState, PermissionState, PersonaState, ProviderState, ToolState,
};
use agnt_core::hook::DoomLoopState;
use agnt_core::hook::output_repetition::RepetitionState;
use agnt_core::protocol::model_switching::ModelSwitching;
use agnt_core::protocol::persona::{FsPersonaResolver, PersonaLister};
use agnt_core::session::StoreType;
use agnt_core::tools::authz::{PermissionsConfig, SessionGrantCache};
use agnt_core::tools::closure::{ClosureRegistry, EngineInterfaceLike};
use agnt_core::tools::handler::McpToolRegistry;
use agnt_core::tools::handler::builtin_tool::{BuiltinTool, ToolRenderRegistry, register_builtin};
use agnt_core::tools::handler::edit::EditTool;
use agnt_core::tools::handler::read::ReadTool;
use agnt_core::tools::mcp::circuit_breaker::McpCircuitBreaker;
use agnt_core::tools::mcp::client::McpToolDefinition;
use agnt_core::tools::mcp::config::{McpConfig, McpServerConfig};
use agnt_core::tools::mcp::runtime::connect_servers;
use agnt_core::types::{
    Completion, DynModel, EmptyToolName, ToolAnnotations, ToolDefinition, ToolName,
};

use crate::engine_stub::AcpEngineStub;
use crate::error::{AcpError, Result};
use crate::fs_tools::{make_acp_edit_tool, make_acp_read_tool};
use crate::session::ClientFsState;

// endregion: --- Modules

// region:    --- Types

/// Inputs required to build an ACP session runtime.
///
/// Every field is resolved by the caller: the session handler owns the ACP
/// request (CWD, client MCP servers, session id) and the process-level
/// resources (tokio handle, session store).
pub struct AcpRuntimeParams {
    /// Session working directory, taken from the ACP `session/new` request.
    pub cwd: PathBuf,
    /// MCP servers declared by the ACP client. These take precedence over
    /// config.toml servers with the same name.
    pub mcp_servers: Vec<McpServerConfig>,
    /// Path to config.toml.
    pub config_path: PathBuf,
    /// Handle to the process tokio runtime.
    pub runtime_handle: tokio::runtime::Handle,
    /// The session store backend the session's rig-memory store is made from.
    pub store_type: StoreType,
    /// Persisted session id, when the session is resumed from the store.
    pub session_id: Option<String>,
    /// The ACP client's filesystem capabilities and connection, when the client
    /// advertised them at `initialize`.
    pub client_fs: Option<ClientFsState>,
    /// Agent whose permissions overlay applies to this session.
    ///
    /// When `Some`, it overrides the default agent resolved from
    /// `agents.default` in config.toml. When `None`, the configured default
    /// agent is used.
    pub agent_name: Option<String>,
}

// endregion: --- Types

// region:    --- Public Functions

/// The id of the model selector config option.
pub const MODEL_CONFIG_ID: &str = "model";

/// The id of the agent selector config option.
pub const AGENT_CONFIG_ID: &str = "agent";

/// Build the model selector advertised to ACP clients.
///
/// With a models cache, every model of the configured provider is selectable.
/// Without one — or when the cache has no entry for the provider — only the
/// currently configured model is advertised, so the client still renders a
/// selector for the active model.
pub fn build_model_config_options(
    config: &Config,
    cache: Option<&ModelsCache>,
) -> Vec<SessionConfigOption> {
    let current = model_value(config);
    let mut options: Vec<SessionConfigSelectOption> = cache
        .map(|cache| {
            cache
                .list_models(Some(config.cache_section()))
                .into_iter()
                // The value is the config's identity, not the cache section
                // key: a block named `ollama` serving the `ollama-cloud`
                // section advertises `ollama/<model>`.
                .map(|(_, model, spec)| {
                    SessionConfigSelectOption::new(
                        format!("{}/{model}", config.provider),
                        spec.name.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !options
        .iter()
        .any(|option| option.value.to_string() == current)
    {
        options.push(SessionConfigSelectOption::new(
            current.clone(),
            config.model.clone(),
        ));
    }
    vec![
        SessionConfigOption::select(MODEL_CONFIG_ID, "Model", current, options)
            .category(SessionConfigOptionCategory::Model),
    ]
}

/// Build the agent selector advertised to ACP clients.
///
/// One value per available persona: the enabled built-ins first, then the
/// filesystem personas of the session's cwd and the XDG config directory.
/// `current` is the session's agent identity; when it is not among the
/// available personas it is added, so the client still renders a selector for
/// the active agent.
pub fn build_agent_config_options(
    agents_config: &AgentsConfig,
    cwd: &Path,
    current: Option<&str>,
) -> Vec<SessionConfigOption> {
    let config_dir = agnt_core::utils::xdg::config_dir()
        .map(|base| base.join("nu-agent"))
        .unwrap_or_else(|_| cwd.to_path_buf());
    let resolver = FsPersonaResolver::new(cwd.to_path_buf(), config_dir, agents_config.clone());
    let mut options: Vec<SessionConfigSelectOption> = resolver
        .list_available()
        .into_iter()
        .map(|summary| SessionConfigSelectOption::new(summary.name.clone(), summary.name))
        .collect();
    let current = current.unwrap_or_default();
    if !current.is_empty()
        && !options
            .iter()
            .any(|option| option.value.to_string() == current)
    {
        options.push(SessionConfigSelectOption::new(
            current.to_string(),
            current.to_string(),
        ));
    }
    vec![
        SessionConfigOption::select(AGENT_CONFIG_ID, "Agent", current.to_string(), options)
            .category(SessionConfigOptionCategory::Mode),
    ]
}

/// Build an `AgentConversationRuntime` for one ACP session.
///
/// Configuration comes from `params.config_path` only — ACP has no CLI flags.
/// Built-in tools are registered directly on a fresh tool server; closure tools
/// are never registered because ACP has no Nushell engine.
pub async fn build_acp_runtime(params: AcpRuntimeParams) -> Result<AgentConversationRuntime> {
    let AcpRuntimeParams {
        cwd,
        mcp_servers,
        config_path,
        runtime_handle,
        store_type,
        session_id,
        client_fs,
        agent_name,
    } = params;

    // -- Config resolution (shared with the plugin path)
    let (config, plugin_config) =
        load_and_resolve_config(&config_path, None).map_err(map_config_load_error)?;
    if let Err(msg) = config.validate() {
        return Err(AcpError::Config(format!("config validation failed: {msg}")));
    }

    let max_tool_result_bytes = config
        .max_tool_result_bytes
        .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES);
    let max_context_tokens = config
        .model_context_tokens
        .or_else(|| config.max_context_tokens.map(|t| t as usize))
        .unwrap_or(defaults::MAX_CONTEXT_TOKENS as usize);
    let repetition_guard = config
        .repetition_guard
        .unwrap_or(defaults::REPETITION_GUARD_ENABLED);
    // The compaction model spec is built once, from the provider name and the
    // limits our config carries.
    let compaction_spec = agnt_core::conversation::compaction::compactor::resolve_model_spec(
        &config.provider,
        &config.model,
        u32::try_from(max_context_tokens).ok(),
        config.max_output_tokens,
    );

    // -- MCP servers: config.toml merged with the ACP client's list
    let config_mcp = McpConfig::from_toml_config(&plugin_config)
        .map_err(|e| AcpError::Config(format!("failed to load MCP config: {e}")))?;
    let merged_mcp = merge_mcp_servers(config_mcp.mcp, mcp_servers);

    // -- Tool server: built-ins only, no closure tools
    let tool_server_handle = agnt_core::types::ToolServer::new().run();
    let bus = agnt_core::bus::create_bus();

    // -- Built-in tools. When the client advertises both filesystem
    // capabilities, the `read` and `edit` tools route through the client's
    // `fs/read_text_file` and `fs/write_text_file` instead of the local
    // filesystem.
    let client_fs_supported = client_fs.as_ref().is_some_and(|state| {
        state.capabilities.read_text_file && state.capabilities.write_text_file
    });
    log::info!(
        "ACP fs tools: client_fs={} supported={} read={} write={}",
        client_fs.is_some(),
        client_fs_supported,
        client_fs
            .as_ref()
            .is_some_and(|s| s.capabilities.read_text_file),
        client_fs
            .as_ref()
            .is_some_and(|s| s.capabilities.write_text_file),
    );

    let mut render_registry = ToolRenderRegistry::default();
    let mut acp_read_def: Option<ToolDefinition> = None;
    let mut acp_edit_def: Option<ToolDefinition> = None;
    for def in builtin_tool_definitions()
        .map_err(|e| AcpError::Runtime(format!("invalid builtin tool name: {e}")))?
    {
        if client_fs_supported && def.name == "read" {
            acp_read_def = Some(def);
            continue;
        }
        if client_fs_supported && def.name == "edit" {
            acp_edit_def = Some(def);
            continue;
        }
        register_builtin(
            def,
            cwd.clone(),
            max_tool_result_bytes,
            bus.clone(),
            &tool_server_handle,
            &mut render_registry,
        )
        .await;
    }

    if let (Some(def), Some(state)) = (acp_read_def, client_fs.as_ref()) {
        render_registry.register(&def.name, ReadTool::call_line_render);
        let tool = make_acp_read_tool(
            def,
            cwd.clone(),
            max_tool_result_bytes,
            state.connection.clone(),
            session_id.clone().unwrap_or_default(),
        );
        tool_server_handle.add_dynamic_tool(tool);
    }

    if let (Some(def), Some(state)) = (acp_edit_def, client_fs.as_ref()) {
        render_registry.register(&def.name, EditTool::call_line_render);
        let tool = make_acp_edit_tool(
            def,
            cwd.clone(),
            max_tool_result_bytes,
            state.connection.clone(),
            session_id.clone().unwrap_or_default(),
        );
        tool_server_handle.add_dynamic_tool(tool);
    }

    let mcp_runtime = if merged_mcp.is_empty() {
        None
    } else {
        Some(
            connect_servers(
                &tool_server_handle,
                &merged_mcp,
                Some(cwd.as_path()),
                max_tool_result_bytes,
                plugin_config.vault.as_ref(),
            )
            .await
            .map_err(|e| AcpError::Runtime(format!("failed to connect MCP servers: {e}")))?,
        )
    };

    let discovered_mcp_tools = mcp_runtime
        .as_ref()
        .map(|runtime| runtime.discovered_tools().to_vec())
        .unwrap_or_default();
    let mcp_lifecycle_projection = mcp_runtime
        .as_ref()
        .map(|runtime| runtime.lifecycle_projection(&merged_mcp))
        .unwrap_or_default();
    let mcp_registry = McpToolRegistry::from_tools(discovered_mcp_tools.clone())
        .map_err(|e| AcpError::Runtime(format!("failed to build MCP tool registry: {e}")))?;

    let mut tool_definitions = builtin_tool_definitions()
        .map_err(|e| AcpError::Runtime(format!("invalid builtin tool name: {e}")))?;
    let mcp_definitions = discovered_mcp_tools
        .iter()
        .map(mcp_tool_definition)
        .collect::<core::result::Result<Vec<_>, EmptyToolName>>()
        .map_err(|e| AcpError::Runtime(format!("invalid MCP tool name: {e}")))?;
    tool_definitions.extend(mcp_definitions);
    let baseline_tool_definitions = tool_definitions.clone();

    // -- Permissions from config.toml only. ACP prompts the client, so the
    // interactive defaults apply. ACP has no CLI permission flags, so the CLI
    // overlay is always `None`. The effective agent's persona overlay is
    // applied by the startup switch below.
    let base_permissions = PermissionsConfig::from_plugin_config(&plugin_config, true);
    let effective_permissions = base_permissions.clone();
    let permissions_summary = effective_permissions.summary();
    let permissions_startup_summary = format!(
        "permissions policy: overlay_active=false global={} tool_rules={} nested_command_rules={}",
        permissions_summary.global.as_str(),
        permissions_summary.tool_rule_count,
        permissions_summary.nested_field_rule_count,
    );

    // -- Preamble cache: AGENTS.md chain and available skills
    let agents_load = agnt_core::protocol::agents::load_agents_chain_for_cwd(&cwd);
    for warning in &agents_load.warnings {
        log::warn!("AGENTS.md load warning: {warning}");
    }
    let cached_agents_chain = agents_load.merged_chain;
    let cached_available_skills =
        agnt_core::protocol::skills::render_available_skills_preamble(&cwd);

    // -- Compaction from config.toml
    let merged_compaction = plugin_config.compaction.clone().unwrap_or_default();
    merged_compaction
        .validate()
        .map_err(|msg| AcpError::Config(format!("compaction config invalid: {msg}")))?;
    let compaction_params = build_compaction_params(&merged_compaction);
    let threshold_pct = merged_compaction
        .proactive_threshold_pct
        .unwrap_or(defaults::PROACTIVE_THRESHOLD_PCT);
    let threshold_tokens = Some((max_context_tokens as f64 * threshold_pct) as usize);

    // -- Model and compactor share one erased model per consumer
    let mut provider = ProviderState::new(config, Some(plugin_config.clone()));
    let dyn_model = provider
        .build_dyn_model()
        .await
        .map_err(|e| AcpError::Runtime(format!("failed to build model: {e}")))?;
    let dyn_model: Arc<Mutex<DynModel<Completion>>> = Arc::new(Mutex::new(dyn_model));
    let compactor_model = provider
        .build_dyn_model()
        .await
        .map_err(|e| AcpError::Runtime(format!("failed to build compactor model: {e}")))?;
    let compactor_model: Arc<Mutex<DynModel<Completion>>> = Arc::new(Mutex::new(compactor_model));
    let compactor = NuCompactor::from_shared_model(
        Arc::clone(&compactor_model),
        bus.clone(),
        rig_memory::CompactionPolicy::default(),
        compaction_spec,
    );

    let engine: Arc<dyn EngineInterfaceLike> =
        Arc::new(AcpEngineStub::new(cwd.to_string_lossy().to_string()));

    // -- Interactive permission bridge: ACP prompts the client, so the runtime
    // must publish `UiEvent::PermissionRequested` and await the bridge's decision.
    let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));

    let mut runtime = AgentConversationRuntime {
        runtime: runtime_handle,
        tool_server_handle,
        provider,
        tools: ToolState::new(
            tool_definitions,
            baseline_tool_definitions,
            ClosureRegistry::default(),
        ),
        mcp_state: McpState::new(
            mcp_runtime,
            mcp_lifecycle_projection,
            merged_mcp,
            Some(cwd.clone()),
            mcp_registry,
            max_tool_result_bytes,
            plugin_config.vault.clone(),
        ),
        engine,
        final_session_id: session_id,
        permission_state: PermissionState::new(
            base_permissions,
            effective_permissions,
            None,
            SessionGrantCache::default(),
            permissions_startup_summary,
        ),
        session: MemoryState::new(agnt_core::session::RigStoreConfig::new(store_type)),
        persona: PersonaState::new(
            None,
            None,
            None,
            None,
            cached_agents_chain,
            cached_available_skills,
            None,
        ),
        multi_agent: MultiAgentState::new(Vec::new(), plugin_config.agents.clone()),
        cwd,
        interactive_pending: Some(pending),
        // ACP has no preview phase: the translator drops `ToolPreview`, so the
        // completion display must reach the client for the diff to appear.
        renders_tool_previews: false,
        circuit_breaker: Arc::new(Mutex::new(McpCircuitBreaker::default())),
        doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
        output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
        repetition_guard,
        last_total_tokens: Arc::new(Mutex::new(None)),
        bus,
        render_registry,
        dyn_model,
        compaction: agnt_core::conversation::compaction::CompactionConfig {
            compactor,
            params: compaction_params,
            threshold_tokens,
        },
    };

    // -- Populate the persona state for the effective agent by running the core
    // switch flow once. This resolves the persona body, applies the persona's
    // model and config overrides, applies the permissions overlay, and sets the
    // agent identity the agent selector reports.
    let effective_agent = match agent_name {
        Some(name) => Some(name),
        None => plugin_config
            .agents
            .resolve_default_agent()
            .map_err(|e| AcpError::Config(format!("failed to resolve default agent: {e}")))?,
    };
    if let Some(name) = effective_agent {
        runtime
            .switch_agent(&name)
            .await
            .map_err(AcpError::Config)?;
    }

    Ok(runtime)
}

// endregion: --- Public Functions

// region:    --- Support

/// The `"provider/model"` identity of a resolved config.
fn model_value(config: &Config) -> String {
    format!("{}/{}", config.provider, config.model)
}

/// Merge config.toml MCP servers with the ACP client's servers.
///
/// A client server replaces a config.toml server with the same name; servers
/// with names unique to either source are all kept.
fn merge_mcp_servers(
    config_servers: Vec<McpServerConfig>,
    client_servers: Vec<McpServerConfig>,
) -> Vec<McpServerConfig> {
    let mut merged = config_servers;
    for client in client_servers {
        match merged.iter_mut().find(|server| server.name == client.name) {
            Some(existing) => *existing = client,
            None => merged.push(client),
        }
    }
    merged
}

fn mcp_tool_definition(
    tool: &McpToolDefinition,
) -> core::result::Result<ToolDefinition, EmptyToolName> {
    Ok(ToolDefinition {
        annotations: ToolAnnotations::default(),
        name: ToolName::new(tool.name.clone())?,
        description: tool
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP tool from server '{}'", tool.server)),
        parameters: tool.parameters.clone().unwrap_or_else(|| {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": {}
                    }
                },
                "required": ["args"]
            })
        }),
    })
}

/// Map a shared config-load error to an ACP error.
fn map_config_load_error(err: agnt_core::config::ConfigLoadError) -> AcpError {
    use agnt_core::config::ConfigLoadError;
    match err {
        ConfigLoadError::NotFound(path) => {
            AcpError::Config(format!("config not found at {}", path.display()))
        }
        ConfigLoadError::Toml(e) => AcpError::Config(format!("failed to load config: {e}")),
        ConfigLoadError::UnknownRole(role) => {
            AcpError::Config(format!("unknown model role: '{role}'"))
        }
        ConfigLoadError::NoDefaultModel => AcpError::Config(
            "no model configured: add a [models.default] block to config.toml".to_string(),
        ),
        ConfigLoadError::Resolve(msg) => {
            AcpError::Config(format!("failed to resolve model: {msg}"))
        }
        ConfigLoadError::Vault(e) => AcpError::Config(e.to_string()),
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/config_options.rs"]
mod config_options_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/support.rs"]
mod runtime_support;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/construction.rs"]
mod runtime_construction;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/mcp.rs"]
mod runtime_mcp;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/client_fs.rs"]
mod runtime_client_fs;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/config_errors.rs"]
mod runtime_config_errors;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/agent_overlay.rs"]
mod runtime_agent_overlay;

#[cfg(all(test, feature = "integration"))]
#[path = "../test/runtime/agent_flag.rs"]
mod runtime_agent_flag;

// endregion: --- Tests
