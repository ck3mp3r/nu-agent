use nu_plugin::{EngineInterface, EvaluatedCall};
use nu_protocol::{LabeledError, Value};
use std::io::IsTerminal;

use super::permissions::resolve_effective_permissions_config;
use super::resolve_policy::resolve_ui_policy;
use super::tool_defs::{ToolAssembly, assemble_tool_definitions};
use super::{
    AgentMode, extract_and_validate_session_flags, extract_tool_timeout, extract_tools_from_call,
    resolve_agent_mode, resolve_config, should_enter_foreground,
};

use nu_agent_core::{
    config::PluginConfig,
    conversation::runtime::AgentConversationRuntime,
    policy::UiPolicy,
    session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
    tools::handler::McpToolRegistry,
};
use nu_agent_tui::RuntimeRunError;
use nu_agent_tui::platform::safety::RestoreRunError;

pub(super) fn run_command(
    agent: &super::Agent,
    engine: &EngineInterface,
    call: &EvaluatedCall,
    input: &Value,
) -> Result<Value, LabeledError> {
    let ui_policy = resolve_ui_policy(call)
        .map_err(|e| LabeledError::new(format!("Failed to resolve UI policy: {e}")))?;

    let cwd = std::path::PathBuf::from(engine.get_current_dir().map_err(|e| {
        LabeledError::new("Failed to resolve working directory")
            .with_label(format!("{e}"), call.head)
    })?);

    // Initialize file logging if --log-level is provided
    if let Some(log_level_str) = call.get_flag::<String>("log-level")? {
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
            let log_dir = nu_agent_core::utils::xdg::state_dir()
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

    let stdin_is_tty = std::io::stdin().is_terminal();
    let stderr_is_tty = std::io::stderr().is_terminal();
    let input_is_nothing = matches!(input, Value::Nothing { .. });
    let mode = resolve_agent_mode(input_is_nothing, stdin_is_tty, stderr_is_tty);

    let _foreground_guard = if should_enter_foreground(mode, stderr_is_tty) {
        if mode.is_tui() {
            // TUI requires foreground — fail hard if it can't be obtained.
            Some(engine.enter_foreground().map_err(|err| {
                LabeledError::new(format!(
                    "Failed to enter foreground for interactive TUI input: {err}"
                ))
            })?)
        } else {
            // Stderr mode — best-effort so Ctrl+C works. Not fatal if it fails
            // (e.g. fully piped environment with no controlling terminal).
            engine.enter_foreground().ok()
        }
    } else {
        None
    };

    // Validate session flags
    let session_id = extract_and_validate_session_flags(call)?;

    // Resolve configuration from all sources with proper precedence:
    // default < env < plugin < flags
    let mut config = resolve_config(engine, call)?;

    // Apply mode-specific defaults for max_tool_turns if not explicitly configured
    // User-specified value (via --max-turns or config file) always wins
    if config.max_tool_turns.is_none() && !mode.is_tui() {
        config.max_tool_turns = Some(20); // Pipeline mode gets 20, TUI stays unlimited (None)
    }

    // Extract tool timeout for ToolExecutor
    let tool_timeout = extract_tool_timeout(call);

    // Extract tools from --tools flag and build ClosureRegistry
    let tools_map = extract_tools_from_call(call)?;
    let mut closure_registry = nu_agent_core::tools::closure::ClosureRegistry::new();
    for (name, closure) in tools_map {
        let params = nu_agent_core::tools::closure::resolve_closure_params(&closure, engine);
        closure_registry.register(
            name,
            nu_agent_core::tools::closure::ResolvedClosure { closure, params },
        );
    }

    let plugin_config_value = engine.get_plugin_config()?;

    let agents_config = plugin_config_value
        .as_ref()
        .and_then(|v| PluginConfig::from_plugin_config(v).ok())
        .map(|c| c.agents)
        .unwrap_or_default();

    let mcp_config = plugin_config_value
        .as_ref()
        .map(nu_agent_core::tools::mcp::config::McpConfig::from_plugin_config)
        .transpose()
        .map_err(|err| {
            LabeledError::new("Failed to load MCP config").with_label(err.to_string(), call.head)
        })?;

    // Load agent persona and resolve identity
    let (agent_name, cli_name) = super::args::extract_agent_flags(call);
    log::debug!("agent flags: agent_name={agent_name:?}, cli_name={cli_name:?}");
    let broker_flags = super::args::extract_broker_flags(call)?;
    log::debug!("broker flags: present={}", broker_flags.is_some());

    let call_has_model_flag = call.get_flag::<Value>("model").ok().flatten().is_some();
    let persona_resolution = super::persona::resolve_persona(
        agent_name,
        cli_name,
        &agents_config,
        &cwd,
        call,
        &mut config,
        call_has_model_flag,
    )?;
    let persona = persona_resolution.persona;
    let agent_identity = persona_resolution.agent_identity;
    let messaging_identity = persona_resolution.messaging_identity;
    let agent_permissions_overlay = persona_resolution.agent_permissions_overlay;

    let (effective_permissions, permissions_startup_summary) =
        resolve_effective_permissions_config(
            call,
            plugin_config_value.as_ref(),
            agent_permissions_overlay.as_ref(),
            mode.is_tui(),
        )?;

    // Create async runtime for LLM and MCP tool execution
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| LabeledError::new(format!("Failed to create async runtime: {}", e)))?;

    // Create the tool server handle ONCE — both builtins and MCP servers register into it
    let tool_server_handle = rig::tool::server::ToolServer::new().run();

    let mcp_runtime = if let Some(cfg) = mcp_config.as_ref() {
        if cfg.mcp.is_empty() {
            None
        } else {
            let caller_cwd_path = cwd.as_path();

            Some(
                runtime
                    .block_on(nu_agent_core::tools::mcp::runtime::connect_servers(
                        &tool_server_handle,
                        &cfg.mcp,
                        Some(caller_cwd_path),
                        config.max_tool_result_bytes.unwrap_or(20_000),
                    ))
                    .map_err(|msg| {
                        LabeledError::new("Failed to connect MCP runtime")
                            .with_label(msg, call.head)
                    })?,
            )
        }
    } else {
        None
    };

    let discovered_mcp_tools = if let Some(mcp_runtime) = mcp_runtime.as_ref() {
        mcp_runtime.discovered_tools().to_vec()
    } else {
        Vec::new()
    };

    let mcp_lifecycle_projection =
        if let (Some(runtime), Some(cfg)) = (mcp_runtime.as_ref(), mcp_config.as_ref()) {
            runtime.lifecycle_projection(&cfg.mcp)
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
        is_orchestrator,
        has_messaging,
    } = assemble_tool_definitions(
        &closure_registry,
        broker_flags.is_some(),
        &agents_config,
        &discovered_mcp_tools,
        &cwd,
    );

    let resolver = DefaultSessionResolver::new(&agent.store);
    let mut session_resolution = resolver.resolve(SessionResolutionInput {
        use_tui: mode.is_tui(),
        session_id,
        cwd: cwd.clone(),
    })?;

    let super::setup::SetupResult {
        mailbox_rx,
        parent_name,
        merged_compaction,
        compaction_strategy,
    } = super::setup::register_tools(super::setup::RegisterToolsInput {
        runtime: &runtime,
        tool_server_handle: &tool_server_handle,
        closure_registry: &closure_registry,
        cwd: &cwd,
        engine,
        call,
        plugin_config_value: plugin_config_value.as_ref(),
        available_agents: &available_agents,
        messaging_identity: messaging_identity.clone(),
        broker_flags,
        is_orchestrator,
        has_messaging,
        tool_timeout,
        session: session_resolution.session.as_mut(),
        max_tool_result_bytes: config.max_tool_result_bytes.unwrap_or(20_000),
    })?;

    let mcp_caller_cwd = cwd.clone();

    // ── Phase 8: Preamble cache ───────────────────────────────────────────
    let (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction) =
        super::runtime_build::build_preamble_cache(&cwd, parent_name.as_deref());

    // ── Phase 9: Runtime construction ────────────────────────────────────
    let context_window_max_tokens = u64::from(config.resolved_max_context_tokens());
    let mut runtime_impl =
        super::runtime_build::build_runtime(super::runtime_build::RuntimeBuildParams {
            runtime,
            config,
            plugin_config_value,
            tool_definitions,
            baseline_tool_definitions,
            closure_registry,
            mcp_runtime,
            tool_server_handle,
            mcp_lifecycle_projection,
            mcp_server_configs: mcp_config
                .as_ref()
                .map(|cfg| cfg.mcp.clone())
                .unwrap_or_default(),
            mcp_caller_cwd: Some(mcp_caller_cwd),
            mcp_registry,
            engine: engine.clone(),
            store: agent.store.clone(),
            final_session_id: session_resolution.final_session_id,
            context_window_max_tokens,
            compaction_threshold_pct: merged_compaction.proactive_threshold_pct.unwrap_or(0.80),
            compaction_strategy,
            effective_permissions,
            permissions_startup_summary,
            persona_body: persona.as_ref().map(|p| p.body.clone()),
            agent_identity,
            agent_description: persona.as_ref().and_then(|p| p.description.clone()),
            cached_agents_chain,
            cached_available_skills,
            cached_sub_agent_instruction,
            mailbox_rx,
            available_agents,
            agents_config,
            cwd: cwd.clone(),
        });
    log::debug!(
        "runtime: agent_persona_body_len={:?}, agent_identity={:?}, agent_description={:?}",
        runtime_impl.persona_state.persona_body_len(),
        runtime_impl.persona_state.agent_identity(),
        runtime_impl.persona_state.agent_description()
    );
    match mode {
        AgentMode::Tui => run_tui_mode(
            &mut runtime_impl,
            input,
            input_is_nothing,
            call.head,
            ui_policy,
            super::mode_execute::TuiHydrationInput {
                should_hydrate: session_resolution.should_hydrate_transcript,
                initial_messages: session_resolution.initial_messages,
                last_total_tokens: session_resolution.last_total_tokens,
            },
        ),
        AgentMode::Stderr => run_stderr_mode(
            &mut runtime_impl,
            input,
            call.head,
            ui_policy,
            stderr_is_tty,
        ),
    }
}

fn run_tui_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    hydration: super::mode_execute::TuiHydrationInput,
) -> Result<Value, LabeledError> {
    super::mode_execute::run_tui_mode(
        runtime_impl,
        input,
        input_is_nothing,
        span,
        ui_policy,
        hydration,
    )
}

fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
) -> Result<Value, LabeledError> {
    super::mode_execute::run_stderr_mode(runtime_impl, input, span, ui_policy, stderr_is_tty)
}

pub(super) fn map_tui_run_result(
    result: Result<Value, RuntimeRunError<LabeledError>>,
) -> Result<Value, LabeledError> {
    match result {
        Ok(value) => Ok(value),
        Err(RuntimeRunError::Enter(err)) => Err(LabeledError::new(format!(
            "Failed to enter TUI terminal lifecycle: {err}"
        ))),
        Err(RuntimeRunError::Run(RestoreRunError::Run(err))) => Err(err),
        Err(RuntimeRunError::Run(RestoreRunError::RunWithRestore {
            run_error,
            restore_error,
        })) => Err(LabeledError::new(format!(
            "TUI run failed and terminal restore failed: run={run_error}, restore={restore_error}"
        ))),
        Err(RuntimeRunError::Run(RestoreRunError::Restore(err))) => Err(LabeledError::new(
            format!("Failed to restore terminal after TUI run: {err}"),
        )),
    }
}
