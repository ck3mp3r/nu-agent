//! The runtime builder: one `AgentConversationRuntime` per ACP session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agnt_kernel::{BuiltinKind, ToolKind};
use agnt_runtime::config::{defaults, load_and_resolve_config};
use agnt_runtime::conversation::builder::builtin_tool_definitions;
use agnt_runtime::conversation::contracts::DecisionSource;
use agnt_runtime::conversation::runtime::{AgentConversationRuntime, PendingPermissions};
use agnt_runtime::conversation::runtime_build::RuntimeBuildParams;
use agnt_runtime::conversation::runtime_parts::RuntimeParts;
use agnt_runtime::conversation::state::memory::MemoryState;
use agnt_runtime::conversation::state::{MultiAgentState, PersonaState, ProviderState, ToolState};
use agnt_runtime::protocol::model_switching::ModelSwitching;
use agnt_runtime::tools::authz::PermissionsConfig;
use agnt_runtime::tools::closure::{ClosureEvaluator, ClosureRegistry};
use agnt_runtime::tools::handler::McpToolRegistry;
use agnt_runtime::tools::handler::builtin_tool::register_builtin;
use agnt_runtime::tools::mcp::config::McpConfig;
use agnt_runtime::tools::mcp::runtime::{CONNECT_DEADLINE, connect_servers};
use agnt_runtime::types::{Completion, DynModel, EmptyToolName, ToolDefinition};

use crate::engine_stub::AcpEngineStub;
use crate::error::{AcpError, Result};
use crate::fs_tools::{make_acp_edit_tool, make_acp_read_tool};

use super::params::AcpRuntimeParams;
use super::support::{map_config_load_error, mcp_tool_definition, merge_mcp_servers};

/// Build an `AgentConversationRuntime` for one ACP session.
///
/// Configuration comes from `params.paths` only — ACP has no CLI flags.
/// Built-in tools are registered directly on a fresh tool server; closure tools
/// are never registered because ACP has no Nushell engine.
pub async fn build_acp_runtime(params: AcpRuntimeParams) -> Result<AgentConversationRuntime> {
    let AcpRuntimeParams {
        cwd,
        mcp_servers,
        paths,
        grammar_stores,
        runtime_handle,
        rig,
        session_id,
        client_fs,
        agent_name,
        hook_context,
    } = params;

    // -- Config resolution (shared with the plugin path)
    let (config, plugin_config) =
        load_and_resolve_config(&paths.config_path, None).map_err(map_config_load_error)?;
    if let Err(msg) = config.validate() {
        return Err(AcpError::Config(format!("config validation failed: {msg}")));
    }

    // The hook envelopes stamp the model the session starts on. The startup
    // `switch_agent` below then writes the persona slug and the model the
    // persona resolves to.
    hook_context.set_model(
        agnt_runtime::protocol::picker::format_active_model_identity(
            &config.provider,
            &config.model,
        ),
    );

    let max_tool_result_bytes = config
        .max_tool_result_bytes
        .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES);

    // -- MCP servers: config.toml merged with the ACP client's list
    let config_mcp = McpConfig::from_toml_config(&plugin_config)
        .map_err(|e| AcpError::Config(format!("failed to load MCP config: {e}")))?;
    let merged_mcp = merge_mcp_servers(config_mcp.mcp, mcp_servers);

    // -- Tool server: built-ins only, no closure tools
    let tool_server_handle = agnt_runtime::types::ToolServer::new().run();
    let bus = agnt_runtime::bus::Bus::default();

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

    let mut acp_read_def: Option<ToolDefinition> = None;
    let mut acp_edit_def: Option<ToolDefinition> = None;
    for def in builtin_tool_definitions()
        .map_err(|e| AcpError::Runtime(format!("invalid builtin tool name: {e}")))?
    {
        if client_fs_supported
            && matches!(
                ToolKind::of_name(&def.name),
                ToolKind::Builtin(BuiltinKind::Read)
            )
        {
            acp_read_def = Some(def);
            continue;
        }
        if client_fs_supported
            && matches!(
                ToolKind::of_name(&def.name),
                ToolKind::Builtin(BuiltinKind::Edit)
            )
        {
            acp_edit_def = Some(def);
            continue;
        }
        register_builtin(
            def,
            cwd.clone(),
            max_tool_result_bytes,
            bus.cancel(),
            Arc::clone(&grammar_stores),
            &tool_server_handle,
        )
        .await;
    }

    if let (Some(def), Some(state)) = (acp_read_def, client_fs.as_ref()) {
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
                CONNECT_DEADLINE,
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
    let agents_load =
        agnt_runtime::protocol::agents::load_agents_chain_for_cwd(&cwd, Some(&paths.agents_dir));
    for warning in &agents_load.warnings {
        log::warn!("AGENTS.md load warning: {warning}");
    }
    let cached_agents_chain = agents_load.merged_chain;
    let cached_available_skills =
        agnt_runtime::protocol::skills::render_available_skills_preamble(&cwd);

    // -- Model and compactor share one erased model per consumer
    let mut provider = ProviderState::new(config.clone(), Some(plugin_config.clone()));
    let (compactor_model, _wire_vendor) = provider
        .build_dyn_model()
        .await
        .map_err(|e| AcpError::Runtime(format!("failed to build compactor model: {e}")))?;
    let compactor_model: Arc<Mutex<DynModel<Completion>>> = Arc::new(Mutex::new(compactor_model));

    let engine: Arc<dyn ClosureEvaluator> = Arc::new(AcpEngineStub);

    // -- Interactive permission bridge: ACP prompts the client, so the runtime
    // must publish `PermissionRecord::Requested` and await the bridge's decision.
    let pending: PendingPermissions = Arc::new(Mutex::new(HashMap::new()));

    let parts = RuntimeParts {
        provider,
        tools: ToolState::new(
            tool_definitions.clone(),
            baseline_tool_definitions.clone(),
            ClosureRegistry::default(),
        ),
        session: MemoryState::new(rig.clone()),
        persona: PersonaState::new(
            None,
            None,
            None,
            None,
            cached_agents_chain.clone(),
            cached_available_skills.clone(),
        ),
        multi: MultiAgentState::new(Vec::new(), plugin_config.agents.clone()),
        engine: Arc::clone(&engine),
        bus: bus.clone(),
    };

    // ACP has no preview phase: the translator drops `ToolPreview`, so the
    // completion display must reach the client for the diff to appear. The card
    // content rides `PermissionRecord` (the resolver mints it, the record names
    // the call), and only the decision source is the interactive bridge here.
    let mut runtime = AgentConversationRuntime::compose(
        parts,
        RuntimeBuildParams {
            runtime: runtime_handle,
            config,
            plugin_config: Some(plugin_config.clone()),
            tool_definitions,
            baseline_tool_definitions,
            closure_registry: ClosureRegistry::default(),
            mcp_runtime,
            tool_server_handle,
            mcp_lifecycle_projection,
            mcp_server_configs: merged_mcp,
            mcp_caller_cwd: Some(cwd.clone()),
            vault: plugin_config.vault.clone(),
            mcp_registry,
            engine,
            rig,
            final_session_id: session_id,
            base_permissions,
            effective_permissions,
            cli_permissions_overlay: None,
            permissions_startup_summary,
            persona_body: None,
            agent_identity: None,
            agent_description: None,
            agent_icon: None,
            cached_agents_chain,
            cached_available_skills,
            available_agents: Vec::new(),
            agents_config: plugin_config.agents.clone(),
            cwd,
            paths,
            bus,
            hook_context,
            compactor_model: Some(compactor_model),
        },
    )
    .await
    .map_err(|e| AcpError::Runtime(e.msg))?
    .with_decision_source(Some(pending));

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
