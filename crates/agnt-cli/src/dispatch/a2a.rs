//! A2A agent server dispatch for the CLI.
//!
//! Runs the full default-mode runtime (config, persona, permissions, tools,
//! MCP, session) with an A2A server in the background. Incoming A2A tasks are
//! injected as prompts into the interactive loop, and A2A tools are registered
//! on the tool server so the agent can talk to peers.

use std::io::IsTerminal;
use std::sync::Arc;

use agnt_a2a::{AgentBuilder, Skill};
use agnt_core::config::defaults;
use agnt_core::conversation::builder::{
    AgentRuntimeBuilder, BuildArtifacts, BuildInput, ToolAssembly, assemble_tool_definitions,
    merge_compaction_configs,
};
use agnt_core::conversation::runtime_build::{
    RuntimeBuildParams, apply_persona_config, apply_persona_model, build_preamble_cache,
    build_runtime,
};
use agnt_core::session::resolver::{
    DefaultSessionResolver, SessionResolutionInput, SessionResolver,
};
use agnt_core::tools::closure::{ClosureRegistry, CwdEngine};
use agnt_core::tools::handler::McpToolRegistry;
use agnt_core::tools::mcp::config::McpConfig;
use agnt_core::types::ToolServer;
use nu_protocol::Span;

use crate::A2aArgs;
use crate::AgentArgs;
use crate::error::{Error, Result};
use crate::mode::{A2aContext, TuiHydrationInput, TuiModeOptions, run_stderr_mode, run_tui_mode};

use super::config::{
    apply_cli_flags_from_clap, extract_compaction_flags_from_clap, extract_tool_timeout_from_clap,
    resolve_config_from_clap, resolve_store_type_from_clap, resolve_theme_from_clap,
    resolve_ui_policy_from_clap,
};
use super::permissions::resolve_effective_permissions_from_clap;
use super::persona::resolve_persona_from_clap;

// region:    --- Types

/// Which runtime mode the A2A subcommand dispatches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliMode {
    /// Interactive TUI.
    Tui,
    /// Non-interactive stderr rendering.
    Stderr,
}

impl CliMode {
    fn is_tui(self) -> bool {
        matches!(self, Self::Tui)
    }
}

// endregion: --- Types

// region:    --- Functions

/// Start the A2A agent server and run the full interactive runtime.
///
/// The A2A subcommand takes no prompt: it always starts the interactive loop.
/// Incoming A2A tasks are injected as prompts, and A2A tools are registered so
/// the agent can talk to peers. The A2A server is shut down after the loop ends.
pub async fn run_a2a(args: &AgentArgs, a2a: &A2aArgs) -> Result<()> {
    let mode = resolve_mode();
    let cwd = std::env::current_dir()
        .map_err(|e| Error::Custom(format!("Failed to resolve working directory: {e}")))?;
    let ui_policy = resolve_ui_policy_from_clap(args);

    let session_id = args.session.clone();

    let (mut config, plugin_config) = resolve_config_from_clap(args)?;

    let store_type = resolve_store_type_from_clap(args)?;

    // Apply mode-specific defaults for max_tool_turns if not explicitly configured.
    // User-specified value (via --max-turns or config file) always wins.
    if config.max_tool_turns.is_none() && !mode.is_tui() {
        config.max_tool_turns = Some(20);
    }

    let tool_timeout = extract_tool_timeout_from_clap(args)?;

    // The CLI has no closure tools.
    let closure_registry = ClosureRegistry::default();

    let agents_config = plugin_config.agents.clone();

    let mcp_config = McpConfig::from_toml_config(&plugin_config)
        .map_err(|e| Error::Custom(format!("Failed to load MCP config: {e}")))?;

    let (agent_name, cli_name) = (args.agent.clone(), args.name.clone());
    let has_explicit_name = cli_name.is_some();

    let cli_model_provided = args.model.is_some();
    let persona_resolution = resolve_persona_from_clap(agent_name, cli_name, &agents_config, &cwd)?;
    let persona = persona_resolution.persona;
    let agent_identity = persona_resolution.agent_identity;
    let messaging_identity = persona_resolution.messaging_identity;
    let agent_permissions_overlay = persona_resolution.agent_permissions_overlay;

    apply_persona_model(
        &mut config,
        Some(&plugin_config),
        persona.as_ref().and_then(|p| p.model.as_deref()),
        cli_model_provided,
    )?;

    if let Some(p) = &persona {
        apply_persona_config(&mut config, p, args.max_turns.is_some());
    }

    apply_cli_flags_from_clap(&mut config, args);

    let (
        base_permissions,
        effective_permissions,
        cli_permissions_overlay,
        permissions_startup_summary,
    ) = resolve_effective_permissions_from_clap(
        &plugin_config,
        agent_permissions_overlay.as_ref(),
        mode.is_tui(),
    );

    // ── A2A agent startup ─────────────────────────────────────────────────
    let a2a_agent_name = messaging_identity.as_deref().unwrap_or("agent");
    let mesh_key = agnt_a2a::mesh_key::resolve_mesh_key(a2a.mesh_key.clone(), &cwd);

    let mut builder = AgentBuilder::new(a2a_agent_name).has_explicit_name(has_explicit_name);
    if let Some(desc) = persona.as_ref().and_then(|p| p.description.as_deref()) {
        builder = builder.description(desc);
    }
    // Populate A2A skills from the agent persona so other agents can see what
    // this agent does via agent.getCard / agent.list.
    if let Some(persona) = &persona {
        let persona_skill = Skill {
            id: persona.name.clone().unwrap_or_default(),
            name: persona.name.clone().unwrap_or_default(),
            description: persona.description.clone().unwrap_or_default(),
            inputs: None,
            outputs: None,
        };
        builder = builder.skills(vec![persona_skill]);
    }

    let mut a2a_handle = match builder.port(a2a.port).mesh_key(mesh_key).build().await {
        Ok(handle) => handle,
        Err((err, Some(server))) => {
            server.shutdown().await;
            return Err(Error::Custom(format!("A2A startup failed: {err}")));
        }
        Err((err, None)) => return Err(Error::Custom(format!("A2A startup failed: {err}"))),
    };

    log::info!(
        "A2A agent '{a2a_agent_name}' listening on {}",
        a2a_handle.server.local_url
    );

    // Create the tool server handle ONCE — both builtins and MCP servers register into it.
    let tool_server_handle = ToolServer::new().run();

    let mcp_runtime = if mcp_config.mcp.is_empty() {
        None
    } else {
        Some(
            agnt_core::tools::mcp::runtime::connect_servers(
                &tool_server_handle,
                &mcp_config.mcp,
                Some(cwd.as_path()),
                config
                    .max_tool_result_bytes
                    .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES),
                plugin_config.vault.as_ref(),
            )
            .await
            .map_err(|msg| Error::Custom(format!("Failed to connect MCP runtime: {msg}")))?,
        )
    };

    let discovered_mcp_tools = mcp_runtime
        .as_ref()
        .map(|runtime| runtime.discovered_tools().to_vec())
        .unwrap_or_default();

    let mcp_lifecycle_projection = mcp_runtime
        .as_ref()
        .map(|runtime| runtime.lifecycle_projection(&mcp_config.mcp))
        .unwrap_or_default();

    let mcp_registry = McpToolRegistry::from_tools(discovered_mcp_tools.clone())
        .map_err(|msg| Error::Custom(format!("Failed to build MCP tool registry: {msg}")))?;

    // A2A is always enabled in this subcommand, so the A2A tool definitions are
    // registered for the LLM.
    let ToolAssembly {
        tool_definitions,
        baseline_tool_definitions,
        available_agents,
    } = assemble_tool_definitions(
        &closure_registry,
        &agents_config,
        &discovered_mcp_tools,
        &cwd,
        true,
    )
    .map_err(|e| Error::Custom(format!("Invalid tool name: {e}")))?;

    let resolver = DefaultSessionResolver::new(agnt_core::session::RigStoreConfig::new(store_type));
    let mut session_resolution = resolver
        .resolve(SessionResolutionInput {
            use_tui: mode.is_tui(),
            session_id,
            cwd: cwd.clone(),
        })
        .await
        .map_err(Error::from)?;

    // Signal bus shared by tool registration and the runtime.
    let bus = agnt_core::bus::create_bus();

    let engine = CwdEngine::shared(cwd.to_string_lossy().to_string());

    let cli_compaction = extract_compaction_flags_from_clap(args)?;
    let merged_compaction =
        merge_compaction_configs(plugin_config.compaction.as_ref(), &cli_compaction);

    let BuildArtifacts {
        parent_name: _,
        merged_compaction,
        compaction_strategy: _,
        compaction_params,
        render_registry,
    } = AgentRuntimeBuilder::new(BuildInput {
        tool_server_handle: &tool_server_handle,
        closure_registry: &closure_registry,
        cwd: cwd.clone(),
        engine: Arc::clone(&engine),
        span: Span::unknown(),
        available_agents: &available_agents,
        messaging_identity: messaging_identity.clone(),
        tool_timeout,
        session: session_resolution.session.as_mut(),
        max_tool_result_bytes: config
            .max_tool_result_bytes
            .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES),
        bus: bus.clone(),
        merged_compaction,
    })
    .build()
    .await
    .map_err(Error::from)?;

    // ── A2A tool registration ─────────────────────────────────────────────
    // Must run BEFORE build_runtime() so tools are available when the agent
    // context snapshots its tool set.
    let a2a_tool_ctx = a2a_handle.a2a_tool_context(tokio::runtime::Handle::current());
    if let Err(e) = agnt_a2a::register_tools_on_server(&tool_server_handle, a2a_tool_ctx).await {
        log::warn!("A2A tool registration failed (non-fatal): {e}");
    }

    let (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction) =
        build_preamble_cache(&cwd, None);

    let mut runtime_impl = build_runtime(RuntimeBuildParams {
        runtime: tokio::runtime::Handle::current(),
        config,
        plugin_config: Some(plugin_config.clone()),
        tool_definitions,
        baseline_tool_definitions,
        closure_registry,
        mcp_runtime,
        tool_server_handle,
        mcp_lifecycle_projection,
        mcp_server_configs: mcp_config.mcp.clone(),
        mcp_caller_cwd: Some(cwd.clone()),
        vault: plugin_config.vault.clone(),
        mcp_registry,
        engine,
        store_type,
        final_session_id: session_resolution.final_session_id.clone(),
        compaction_params,
        proactive_threshold_pct: merged_compaction.proactive_threshold_pct,
        base_permissions,
        effective_permissions,
        cli_permissions_overlay,
        permissions_startup_summary,
        persona_body: persona.as_ref().map(|p| p.body.clone()),
        agent_identity,
        agent_description: persona.as_ref().and_then(|p| p.description.clone()),
        agent_icon: persona.as_ref().and_then(|p| p.icon.clone()),
        cached_agents_chain,
        cached_available_skills,
        cached_sub_agent_instruction,
        available_agents,
        agents_config,
        cwd: cwd.clone(),
        bus: bus.clone(),
        render_registry,
    })
    .await
    .map_err(Error::from)?;

    // ── Extract A2A receivers into the mode context ──────────────────────
    // The receivers are moved into the mode function, which polls them to
    // inject incoming A2A tasks as prompts and to auto-complete them.
    let a2a_context = A2aContext {
        task_rx: a2a_handle.server.take_incoming_task_receiver(),
        completion_rx: a2a_handle.take_completion_receiver(),
        task_cancel_rx: a2a_handle.take_task_cancel_receiver(),
        task_store: Some(a2a_handle.task_store()),
        card_handle: a2a_handle.card_handle(),
        cache: Some(a2a_handle.cache()),
        self_port: Some(a2a_handle.server.port),
        discovery: Some(a2a_handle.discovery_handle()),
        mesh_key: Some(a2a_handle.mesh_key().to_string()),
    };

    let result = match mode {
        CliMode::Tui => {
            let theme_name = resolve_theme_from_clap(a2a.ui.theme.as_deref(), &plugin_config)?;
            run_tui_mode(
                runtime_impl,
                None,
                None,
                ui_policy,
                TuiHydrationInput {
                    should_hydrate: session_resolution.should_hydrate_transcript,
                    initial_messages: session_resolution.initial_messages,
                    last_total_tokens: session_resolution.last_total_tokens,
                },
                TuiModeOptions {
                    a2a: a2a_context,
                    theme_name,
                },
            )
            .await
            .map(|_| ())
        }
        CliMode::Stderr => {
            let stderr_is_tty = std::io::stderr().is_terminal();
            run_stderr_mode(
                &mut runtime_impl,
                None,
                None,
                ui_policy,
                stderr_is_tty,
                a2a_context,
            )
            .await
            .map(|_| ())
        }
    };

    // Shut down the A2A agent after the interactive loop ends.
    log::info!("Shutting down A2A agent...");
    a2a_handle.shutdown().await;

    result
}

// endregion: --- Functions

// region:    --- Support

/// Select the execution mode from the terminal state.
///
/// The A2A subcommand never receives a prompt, so the input is always empty:
/// TUI when both stdin and stderr are terminals; stderr rendering otherwise.
fn resolve_mode() -> CliMode {
    use agnt_core::conversation::mode_logic::{AgentMode, resolve_agent_mode};
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stderr_is_tty = std::io::stderr().is_terminal();
    match resolve_agent_mode(true, stdin_is_tty, stderr_is_tty) {
        AgentMode::Tui => CliMode::Tui,
        AgentMode::Stderr => CliMode::Stderr,
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/dispatch/a2a.rs"]
mod a2a_test;

// endregion: --- Tests
