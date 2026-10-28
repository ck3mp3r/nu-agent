//! Agent runtime construction and mode dispatch for the CLI.

use std::io::IsTerminal;
use std::sync::Arc;

use agnt_core::config::defaults;
use agnt_core::conversation::builder::{
    AgentRuntimeBuilder, BuildArtifacts, BuildInput, ToolAssembly, assemble_tool_definitions,
    merge_compaction_configs,
};
use agnt_core::conversation::runtime_build::{
    RuntimeBuildParams, apply_persona_config, apply_persona_model, build_preamble_cache,
    build_runtime,
};
use agnt_core::hooks::HookAgentContext;
use agnt_core::session::resolver::{
    DefaultSessionResolver, SessionResolutionInput, SessionResolver,
};
use agnt_core::tools::closure::{ClosureRegistry, CwdEngine};
use agnt_core::tools::handler::McpToolRegistry;
use agnt_core::tools::mcp::config::McpConfig;
use agnt_core::types::ToolServer;
use nu_protocol::Span;

use crate::AgentArgs;
use crate::UiArgs;
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

/// Which runtime mode the CLI dispatches to.
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

/// Build the runtime from clap args and dispatch to the selected mode.
///
/// `prompt` is `Some` for a single turn (positional arg or piped stdin) and
/// `None` for the interactive TUI.
pub async fn run_agent(args: &AgentArgs, prompt: Option<String>, ui: &UiArgs) -> Result<()> {
    let mode = resolve_mode(prompt.is_none());
    let cwd = std::env::current_dir()
        .map_err(|e| Error::Custom(format!("Failed to resolve working directory: {e}")))?;
    let ui_policy = resolve_ui_policy_from_clap(args);

    let session_id = args.session.clone();

    // Discovery comes first: the file the ladder selects carries the `[paths]`
    // values the full resolution below applies.
    let discovery = agnt_core::config::Paths::resolve_for_process(None)
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    let (mut config, plugin_config) = resolve_config_from_clap(args, &discovery.config_path)?;

    let store_type = resolve_store_type_from_clap(args, config.session_store_type)?;
    let paths = agnt_core::config::Paths::resolve_for_process(plugin_config.paths.as_ref())
        .map_err(|e| Error::Custom(format!("Failed to resolve application paths: {e}")))?;
    // `[paths].state_home` and `[paths].cache_home` only become known now: the
    // pre-load window logged to the derived default, and the grammar cache
    // keeps the OS default until this install.
    agnt_core::logging::retarget_process_sink(&paths.log_path);
    agnt_core::tools::handler::tree_sitter::install_grammar_cache_dir(&paths.grammar_cache_dir);
    let theme_path = paths.theme_path.clone();
    let rig = agnt_core::session::RigStoreConfig::new(store_type)
        .with_store_path(paths.store_path.clone());

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

    let cli_model_provided = args.model.is_some();
    let persona_resolution = resolve_persona_from_clap(
        agent_name,
        cli_name,
        &agents_config,
        &cwd,
        &paths.agents_dir,
    )?;
    let persona = persona_resolution.persona;
    let agent_slug = persona_resolution.agent_slug;
    let agent_identity = persona_resolution.agent_identity;
    let agent_permissions_overlay = persona_resolution.agent_permissions_overlay;

    // The hook envelopes stamp the persona slug; the runtime fills in the model
    // on construction and refreshes both cells on every switch.
    let hook_context = HookAgentContext::default().with_persona(agent_slug);

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

    // The default mode never starts an A2A server, so A2A tools are not registered.
    let ToolAssembly {
        tool_definitions,
        baseline_tool_definitions,
        available_agents,
    } = assemble_tool_definitions(
        &closure_registry,
        &agents_config,
        &discovered_mcp_tools,
        &cwd,
        &paths.agents_dir,
        false,
    )
    .map_err(|e| Error::Custom(format!("Invalid tool name: {e}")))?;

    let resolver = DefaultSessionResolver::new(rig.clone());
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
        tool_timeout,
        session: session_resolution.session.as_mut(),
        max_tool_result_bytes: config
            .max_tool_result_bytes
            .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES),
        bus: bus.clone(),
        merged_compaction,
        audit_log_path: paths.audit_log_path.clone(),
    })
    .build()
    .await
    .map_err(Error::from)?;

    let (cached_agents_chain, cached_available_skills) =
        build_preamble_cache(&cwd, &paths.agents_dir);

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
        rig,
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
        available_agents,
        agents_config,
        cwd: cwd.clone(),
        paths,
        bus: bus.clone(),
        render_registry,
        hook_context,
    })
    .await
    .map_err(Error::from)?;

    match mode {
        CliMode::Tui => {
            let theme_name =
                resolve_theme_from_clap(ui.theme.as_deref(), &plugin_config, &theme_path)?;
            run_tui_mode(
                runtime_impl,
                prompt,
                None,
                ui_policy,
                TuiHydrationInput {
                    should_hydrate: session_resolution.should_hydrate_transcript,
                    initial_messages: session_resolution.initial_messages,
                    last_total_tokens: session_resolution.last_total_tokens,
                },
                TuiModeOptions {
                    a2a: A2aContext::default(),
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
                prompt,
                None,
                ui_policy,
                stderr_is_tty,
                A2aContext::default(),
            )
            .await
            .map(|_| ())
        }
    }
}

// endregion: --- Functions

// region:    --- Support

/// Select the execution mode from the terminal state and prompt presence.
///
/// Mirrors the plugin: TUI when no prompt is piped and both stdin and stderr
/// are terminals; stderr rendering otherwise.
fn resolve_mode(input_is_nothing: bool) -> CliMode {
    use agnt_core::conversation::mode_logic::{AgentMode, resolve_agent_mode};
    let stdin_is_tty = std::io::stdin().is_terminal();
    let stderr_is_tty = std::io::stderr().is_terminal();
    match resolve_agent_mode(input_is_nothing, stdin_is_tty, stderr_is_tty) {
        AgentMode::Tui => CliMode::Tui,
        AgentMode::Stderr => CliMode::Stderr,
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/dispatch/agent.rs"]
mod agent_test;

// endregion: --- Tests
