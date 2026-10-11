use std::sync::Arc;

use nu_plugin::{EngineInterface, EvaluatedCall};
use nu_protocol::{LabeledError, Value};

use super::permissions::resolve_effective_permissions_config;
use super::tool_defs::{ToolAssembly, assemble_tool_definitions};
use super::{
    extract_and_validate_session_flags, extract_tool_timeout, extract_tools_from_call,
    resolve_config,
};

use crate::plugin::AgentPlugin;
use agnt_core::{
    config::defaults,
    conversation::{
        builder::BuildInput,
        runtime_build::{RuntimeBuildParams, build_runtime},
    },
    session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
    tools::{closure::EngineInterfaceLike, handler::McpToolRegistry},
};

/// Run the `agent` command in pipeline mode.
///
/// Pipeline mode consumes a piped prompt (string or record) and returns the
/// turn result as a Nushell `Value`. TUI, TTY, ACP, and A2A dispatch live in the
/// `agent` CLI; the plugin owns only the pipeline path.
pub(super) fn run_command(
    _agent: &super::Agent,
    plugin: &AgentPlugin,
    engine: &EngineInterface,
    call: &EvaluatedCall,
    input: &Value,
) -> Result<Value, LabeledError> {
    let engine_arc: Arc<dyn EngineInterfaceLike> = Arc::new(engine.clone());

    let cwd = std::path::PathBuf::from(engine.get_current_dir().map_err(|e| {
        LabeledError::new("Failed to resolve working directory")
            .with_label(format!("{e}"), call.head)
    })?);

    // Initialize file logging. An explicit --log-level wins; otherwise
    // --verbose raises the level and --quiet lowers it.
    if let Some(log_level_str) = resolve_log_level_str(call)? {
        let log_level_str = log_level_str.to_lowercase();
        if log_level_str != "off" {
            // Parse log level
            let log_level = match log_level_str.as_str() {
                "error" => log::LevelFilter::Error,
                "warn" => log::LevelFilter::Warn,
                "info" => log::LevelFilter::Info,
                "debug" => log::LevelFilter::Debug,
                "trace" => log::LevelFilter::Trace,
                _ => {
                    return Err(LabeledError::new(format!(
                        "Invalid log level '{}'. Valid values: off, error, warn, info, debug, trace",
                        log_level_str
                    )));
                }
            };

            // Resolve log directory: $XDG_STATE_HOME/nu-agent/logs
            let log_dir = agnt_core::utils::xdg::state_dir()
                .map_err(|e| {
                    LabeledError::new(format!("Failed to resolve XDG state directory: {e}"))
                })?
                .join("nu-agent")
                .join("logs");

            // Create log directory
            std::fs::create_dir_all(&log_dir).map_err(|e| {
                LabeledError::new(format!(
                    "Failed to create log directory {}: {}",
                    log_dir.display(),
                    e
                ))
            })?;

            // Open log file in append mode
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("agent.log"))
                .map_err(|e| LabeledError::new(format!("Failed to open log file: {e}")))?;

            // Initialize env_logger with file target
            let _ = env_logger::Builder::new()
                .filter_level(log_level)
                .target(env_logger::Target::Pipe(Box::new(log_file)))
                .format_timestamp_millis()
                .try_init();
            // Always force the global max level — try_init() is a no-op when a
            // logger is already registered (e.g. by nu-plugin internals),
            // silently discarding our filter_level. set_max_level() always wins.
            log::set_max_level(log_level);
        }
    }

    // Validate session flags
    let session_id = extract_and_validate_session_flags(call)?;

    // Resolve configuration from all sources with proper precedence:
    // 1. Resolve default role config (--model flag handled here)
    let (mut config, plugin_config) = resolve_config(call)?;

    // Resolve session store type with CLI > env > config > default precedence
    let store_type = plugin.resolve_store_type(call)?;
    let store = Arc::new(
        plugin
            .create_store_with(store_type)
            .map_err(|e| LabeledError::new(format!("Failed to create session store: {e}")))?,
    );

    // Pipeline mode gets a bounded tool-turn budget unless explicitly configured.
    // User-specified value (via --max-turns or config file) always wins.
    if config.max_tool_turns.is_none() {
        config.max_tool_turns = Some(20);
    }

    // Extract tool timeout for ToolExecutor
    let tool_timeout = extract_tool_timeout(call);

    // Extract tools from --tools flag and build ClosureRegistry
    let tools_map = extract_tools_from_call(call)?;
    let mut closure_registry = agnt_core::tools::closure::ClosureRegistry::default();
    for (name, closure) in tools_map {
        let params = agnt_core::tools::closure::resolve_closure_params(&closure, engine);
        closure_registry.register(
            name,
            agnt_core::tools::closure::ResolvedClosure { closure, params },
        );
    }

    let agents_config = plugin_config.agents.clone();

    let mcp_config = agnt_core::tools::mcp::config::McpConfig::from_toml_config(&plugin_config)
        .map_err(|err| LabeledError::new("Failed to load MCP config").with_label(err, call.head))?;

    // Load agent persona and resolve identity
    let (agent_name, cli_name) = super::args::extract_agent_flags(call);
    log::debug!("agent flags: agent_name={agent_name:?}, cli_name={cli_name:?}");

    let call_has_model_flag = call.get_flag::<Value>("model").ok().flatten().is_some();
    let persona_resolution = super::persona::resolve_persona(
        agent_name,
        cli_name,
        &agents_config,
        &cwd,
        call,
        &mut config,
    )?;
    let persona = persona_resolution.persona;
    let agent_identity = persona_resolution.agent_identity;
    let messaging_identity = persona_resolution.messaging_identity;
    let agent_permissions_overlay = persona_resolution.agent_permissions_overlay;

    // 3. Apply persona model (resolves heavy/light role, replaces config)
    super::runtime_build::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        persona.as_ref().and_then(|p| p.model.as_deref()),
        call_has_model_flag,
    )?;

    // 4. Apply persona config (front matter overrides)
    if let Some(p) = &persona {
        let cli_max_turns_provided = call.get_flag::<Value>("max-turns").ok().flatten().is_some();
        super::runtime_build::apply_persona_config(&mut config, p, cli_max_turns_provided);
    }

    // 5. Apply CLI flags LAST (highest priority)
    super::runtime_build::apply_cli_flags(&mut config, call);

    let (
        base_permissions,
        effective_permissions,
        cli_permissions_overlay,
        permissions_startup_summary,
    ) = resolve_effective_permissions_config(
        call,
        &plugin_config,
        agent_permissions_overlay.as_ref(),
        false,
    )?;

    // Use the shared runtime from AgentPlugin — created once at plugin startup,
    // reused for all command invocations.
    let handle = plugin.runtime()?.handle().clone();
    handle.clone().block_on(async move {
        // Create the tool server handle ONCE — both builtins and MCP servers register into it
        let tool_server_handle = agnt_core::types::ToolServer::new().run();

        let mcp_runtime = if mcp_config.mcp.is_empty() {
            None
        } else {
            let caller_cwd_path = cwd.as_path();

            Some(
                agnt_core::tools::mcp::runtime::connect_servers(
                    &tool_server_handle,
                    &mcp_config.mcp,
                    Some(caller_cwd_path),
                    config
                        .max_tool_result_bytes
                        .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES),
                    plugin_config.vault.as_ref(),
                )
                .await
                .map_err(|msg| {
                    LabeledError::new("Failed to connect MCP runtime").with_label(msg, call.head)
                })?,
            )
        };

        let discovered_mcp_tools = if let Some(mcp_runtime) = mcp_runtime.as_ref() {
            mcp_runtime.discovered_tools().to_vec()
        } else {
            Vec::new()
        };

        let mcp_lifecycle_projection = if let Some(runtime) = mcp_runtime.as_ref() {
            runtime.lifecycle_projection(&mcp_config.mcp)
        } else {
            Vec::new()
        };

        let mcp_registry =
            McpToolRegistry::from_tools(discovered_mcp_tools.clone()).map_err(|msg| {
                LabeledError::new("Failed to build MCP tool registry").with_label(msg, call.head)
            })?;

        let ToolAssembly {
            tool_definitions,
            baseline_tool_definitions,
            available_agents,
        } = assemble_tool_definitions(
            &closure_registry,
            &agents_config,
            &discovered_mcp_tools,
            &cwd,
            false,
        );

        let resolver = DefaultSessionResolver::new(Arc::clone(&store));
        let mut session_resolution = resolver
            .resolve(SessionResolutionInput {
                use_tui: false,
                session_id,
                cwd: cwd.clone(),
            })
            .await?;

        // Signal bus shared by tool registration and the runtime.
        let bus = agnt_core::bus::create_bus();

        let agnt_core::conversation::builder::BuildArtifacts {
            parent_name: _,
            merged_compaction,
            compaction_strategy: _,
            compaction_params,
            render_registry,
        } = super::setup::register_tools(
            call,
            BuildInput {
                tool_server_handle: &tool_server_handle,
                closure_registry: &closure_registry,
                cwd: cwd.clone(),
                engine: engine_arc.clone(),
                span: call.head,
                available_agents: &available_agents,
                messaging_identity: messaging_identity.clone(),
                tool_timeout,
                session: session_resolution.session.as_mut(),
                max_tool_result_bytes: config
                    .max_tool_result_bytes
                    .unwrap_or(defaults::MAX_TOOL_RESULT_BYTES),
                bus: bus.clone(),
                merged_compaction: agnt_core::config::CompactionConfig::default(),
            },
        )
        .await?;

        let mcp_caller_cwd = cwd.clone();

        // ── Preamble cache ────────────────────────────────────────────────────
        let (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction) =
            super::runtime_build::build_preamble_cache(&cwd, None);

        // ── Runtime construction ──────────────────────────────────────────────
        let mut runtime_impl = build_runtime(RuntimeBuildParams {
            runtime: handle.clone(),
            config,
            plugin_config: Some(plugin_config.clone()),
            tool_definitions,
            baseline_tool_definitions,
            closure_registry,
            mcp_runtime,
            tool_server_handle,
            mcp_lifecycle_projection,
            mcp_server_configs: mcp_config.mcp.clone(),
            mcp_caller_cwd: Some(mcp_caller_cwd),
            vault: plugin_config.vault.clone(),
            mcp_registry,
            engine: engine_arc.clone(),
            store: store.clone(),
            final_session_id: session_resolution.final_session_id,
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
        })?;
        log::debug!(
            "runtime: agent_persona_body_len={:?}, agent_identity={:?}, agent_description={:?}",
            runtime_impl.persona_body_len(),
            runtime_impl.agent_identity(),
            runtime_impl.agent_description()
        );

        // ── Pipeline turn ──────────────────────────────────────────────────────
        let (prompt, context) = super::input::extract_prompt_and_context(input)?;
        agnt_core::orchestrator::run_single_turn(
            &mut runtime_impl,
            &bus,
            prompt,
            context,
            call.head,
        )
        .await
    })
}

// region:    --- Support

/// Resolve the log level string from `--log-level`, `--verbose`, and `--quiet`.
///
/// An explicit `--log-level` always wins. Otherwise `--quiet` selects `error`
/// and `--verbose` selects `debug`. With none of the flags set, logging stays
/// at the default level and no file logger is initialized.
fn resolve_log_level_str(call: &EvaluatedCall) -> Result<Option<String>, LabeledError> {
    if let Some(level) = call.get_flag::<String>("log-level")? {
        return Ok(Some(level));
    }
    if call.has_flag("quiet")? {
        return Ok(Some("error".to_string()));
    }
    if call.has_flag("verbose")? {
        return Ok(Some("debug".to_string()));
    }
    Ok(None)
}

// endregion: --- Support
