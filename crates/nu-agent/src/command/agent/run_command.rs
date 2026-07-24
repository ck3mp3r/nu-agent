use std::io::IsTerminal;
use std::sync::Arc;

use nu_plugin::{EngineInterface, EvaluatedCall};
use nu_protocol::{LabeledError, Value};

use nu_agent_a2a::InMemoryTaskStore;

use super::permissions::resolve_effective_permissions_config;
use super::resolve_policy::resolve_ui_policy;
use super::tool_defs::{ToolAssembly, assemble_tool_definitions};
use super::{
    AgentMode, extract_and_validate_session_flags, extract_tool_timeout, extract_tools_from_call,
    resolve_agent_mode, resolve_config, should_enter_foreground,
};

use crate::plugin::AgentPlugin;
use nu_agent_a2a::{AgentBuilder, AgentHandle, Skill};
use nu_agent_core::{
    config::PluginConfig,
    conversation::{builder::BuildInput, runtime::AgentConversationRuntime},
    policy::UiPolicy,
    session::resolver::{DefaultSessionResolver, SessionResolutionInput, SessionResolver},
    tools::handler::McpToolRegistry,
};

pub(super) fn run_command(
    _agent: &super::Agent,
    plugin: &AgentPlugin,
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

    // --a2a-port enables A2A and optionally sets the port
    if let Some(port_val) = call.get_flag::<i64>("a2a-port")? {
        if !(0..=65535).contains(&port_val) {
            return Err(LabeledError::new(format!(
                "Invalid --a2a-port value {port_val}. Must be between 0 and 65535."
            )));
        }
        config.a2a_enabled = true;
        config.a2a_port = Some(port_val as u16);
    }

    // Resolve session store type with CLI > env > config > default precedence
    let store_type = plugin.resolve_store_type(call)?;
    let store = Arc::new(
        plugin
            .create_store_with(store_type)
            .map_err(|e| LabeledError::new(format!("Failed to create session store: {e}")))?,
    );

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

    let agents_config = match plugin_config_value.as_ref() {
        Some(v) => match PluginConfig::from_plugin_config(v) {
            Ok(c) => c.agents,
            Err(e) => {
                log::warn!("failed to parse plugin config: {e}");
                Default::default()
            }
        },
        None => Default::default(),
    };

    let mcp_config = plugin_config_value
        .as_ref()
        .map(nu_agent_core::tools::mcp::config::McpConfig::from_plugin_config)
        .transpose()
        .map_err(|err| {
            LabeledError::new("Failed to load MCP config").with_label(err.to_string(), call.head)
        })?;

    // Load agent persona and resolve identity
    let (agent_name, cli_name) = super::args::extract_agent_flags(call);
    let has_explicit_name = cli_name.is_some();
    log::debug!("agent flags: agent_name={agent_name:?}, cli_name={cli_name:?}");

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

    let (
        base_permissions,
        effective_permissions,
        cli_permissions_overlay,
        permissions_startup_summary,
    ) = resolve_effective_permissions_config(
        call,
        plugin_config_value.as_ref(),
        agent_permissions_overlay.as_ref(),
        mode.is_tui(),
    )?;

    // Use the shared runtime from AgentPlugin — created once at plugin startup,
    // reused for all command invocations. The handle is cloned for use inside
    // the async block and for spawning tasks (ToolServer, A2A, etc.).
    let handle = plugin.runtime.handle().clone();
    handle.clone().block_on(async move {
        // ── A2A agent startup (optional, experimental) ───────────────────────
        let mut a2a_handle: Option<AgentHandle> = if config.a2a_enabled {
            let agent_name = messaging_identity.as_deref().unwrap_or("agent");
            let description = persona.as_ref().and_then(|p| p.description.as_deref());
            let a2a_port = config.a2a_port.unwrap_or(0);
            let explicit_mesh_key = call.get_flag::<String>("mesh-key")?;
            let mesh_key = nu_agent_a2a::mesh_key::resolve_mesh_key(explicit_mesh_key, &cwd);
            match async {
                let mut builder =
                    AgentBuilder::new(agent_name).has_explicit_name(has_explicit_name);
                if let Some(desc) = description {
                    builder = builder.description(desc);
                }
                // Populate A2A skills from the agent persona so other agents can
                // see what this agent does via agent.getCard / agent.list.
                if let Some(ref persona) = persona {
                    let persona_skill = Skill {
                        id: persona.name.clone().unwrap_or_default(),
                        name: persona.name.clone().unwrap_or_default(),
                        description: persona.description.clone().unwrap_or_default(),
                        inputs: None,
                        outputs: None,
                    };
                    builder = builder.skills(vec![persona_skill]);
                }
                builder.port(a2a_port).mesh_key(mesh_key).build().await
            }
            .await
            {
                Ok(handle) => {
                    log::info!(
                        "A2A agent '{agent_name}' started on {}",
                        handle.server.local_url
                    );
                    Some(handle)
                }
                Err((err, Some(server))) => {
                    server.shutdown().await;
                    log::warn!("A2A startup failed (non-fatal): {err}");
                    None
                }
                Err((err, None)) => {
                    log::warn!("A2A startup failed (non-fatal): {err}");
                    None
                }
            }
        } else {
            None
        };

        // Create the tool server handle ONCE — both builtins and MCP servers register into it
        let tool_server_handle = rig::tool::server::ToolServer::new().run();

        let mcp_runtime = if let Some(cfg) = mcp_config.as_ref() {
            if cfg.mcp.is_empty() {
                None
            } else {
                let caller_cwd_path = cwd.as_path();

                Some(
                    nu_agent_core::tools::mcp::runtime::connect_servers(
                        &tool_server_handle,
                        &cfg.mcp,
                        Some(caller_cwd_path),
                        config.max_tool_result_bytes.unwrap_or(20_000),
                    )
                    .await
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
        } = assemble_tool_definitions(
            &closure_registry,
            &agents_config,
            &discovered_mcp_tools,
            &cwd,
        );

        let resolver = DefaultSessionResolver::new(Arc::clone(&store));
        let mut session_resolution = resolver
            .resolve(SessionResolutionInput {
                use_tui: mode.is_tui(),
                session_id,
                cwd: cwd.clone(),
            })
            .await?;

        let nu_agent_core::conversation::builder::BuildArtifacts {
            parent_name: _,
            merged_compaction,
            compaction_strategy,
            compaction_params,
        } = super::setup::register_tools(
            call,
            plugin_config_value.as_ref(),
            BuildInput {
                tool_server_handle: &tool_server_handle,
                closure_registry: &closure_registry,
                cwd: cwd.clone(),
                engine,
                span: call.head,
                available_agents: &available_agents,
                messaging_identity: messaging_identity.clone(),
                tool_timeout,
                session: session_resolution.session.as_mut(),
                max_tool_result_bytes: config.max_tool_result_bytes.unwrap_or(20_000),
                merged_compaction: nu_agent_core::config::CompactionConfig::default(),
            },
        )
        .await?;

        // ── A2A tool registration ──────────────────────────────────────────────
        // Must run BEFORE build_runtime() so tools are available when the agent
        // context snapshots its tool set.
        if let Some(ref a2a_h) = a2a_handle {
            let ctx = a2a_h.a2a_tool_context(handle.clone());
            if let Err(e) = nu_agent_a2a::register_tools_on_server(&tool_server_handle, ctx).await {
                log::warn!("A2A tool registration failed (non-fatal): {e}");
            }
        }

        let mcp_caller_cwd = cwd.clone();

        // ── Phase 8: Preamble cache ───────────────────────────────────────────
        let (cached_agents_chain, cached_available_skills, cached_sub_agent_instruction) =
            super::runtime_build::build_preamble_cache(&cwd, None);

        // ── Phase 9: Runtime construction ────────────────────────────────────
        let context_window_max_tokens = u64::from(config.resolved_max_context_tokens());
        let mut runtime_impl =
            super::runtime_build::build_runtime(super::runtime_build::RuntimeBuildParams {
                runtime: handle.clone(),
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
                store: store.clone(),
                final_session_id: session_resolution.final_session_id,
                context_window_max_tokens,
                compaction_threshold_pct: merged_compaction.proactive_threshold_pct.unwrap_or(0.80),
                compaction_strategy,
                compaction_params,
                base_permissions,
                effective_permissions,
                cli_permissions_overlay,
                permissions_startup_summary,
                persona_body: persona.as_ref().map(|p| p.body.clone()),
                agent_identity,
                agent_description: persona.as_ref().and_then(|p| p.description.clone()),
                cached_agents_chain,
                cached_available_skills,
                cached_sub_agent_instruction,
                available_agents,
                agents_config,
                cwd: cwd.clone(),
            });
        log::debug!(
            "runtime: agent_persona_body_len={:?}, agent_identity={:?}, agent_description={:?}",
            runtime_impl.persona_body_len(),
            runtime_impl.agent_identity(),
            runtime_impl.agent_description()
        );

        // Extract the A2A incoming task receiver (if A2A is enabled).
        // The receiver is moved into the mode function where it will be polled
        // before each interactive turn to inject A2A tasks as user prompts.
        let a2a_task_rx = a2a_handle
            .as_mut()
            .and_then(|h| h.server.take_incoming_task_receiver());

        // Extract the A2A completion event receiver (if A2A is enabled).
        // This receives events when remote agents finish processing tasks
        // that were sent via tasks.send.
        let a2a_completion_rx = a2a_handle
            .as_mut()
            .and_then(|h| h.take_completion_receiver());

        // Extract a reference to the A2A task store for auto-completing incoming
        // tasks when the LLM finishes processing them.
        let a2a_task_store: Option<Arc<InMemoryTaskStore>> =
            a2a_handle.as_ref().map(|h| h.task_store());

        let a2a = super::mode_execute::A2aContext {
            task_rx: a2a_task_rx,
            completion_rx: a2a_completion_rx,
            task_store: a2a_task_store.clone(),
        };

        let result = match mode {
            AgentMode::Tui => {
                run_tui_mode(
                    runtime_impl,
                    input,
                    input_is_nothing,
                    call.head,
                    ui_policy,
                    super::mode_execute::TuiHydrationInput {
                        should_hydrate: session_resolution.should_hydrate_transcript,
                        initial_messages: session_resolution.initial_messages,
                        last_total_tokens: session_resolution.last_total_tokens,
                    },
                    a2a,
                )
                .await
            }
            AgentMode::Stderr => {
                run_stderr_mode(
                    &mut runtime_impl,
                    input,
                    call.head,
                    ui_policy,
                    stderr_is_tty,
                    a2a,
                )
                .await
            }
        };

        // Shutdown A2A agent before returning (catch panics, never crash agent)
        if let Some(handle) = a2a_handle {
            log::info!("Shutting down A2A agent...");
            let _ = handle.shutdown().await;
        }

        result
    })
}

async fn run_tui_mode(
    runtime_impl: AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    hydration: super::mode_execute::TuiHydrationInput,
    a2a: super::mode_execute::A2aContext,
) -> Result<Value, LabeledError> {
    super::mode_execute::run_tui_mode(
        runtime_impl,
        input,
        input_is_nothing,
        span,
        ui_policy,
        hydration,
        a2a,
    )
    .await
}

async fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    a2a: super::mode_execute::A2aContext,
) -> Result<Value, LabeledError> {
    super::mode_execute::run_stderr_mode(runtime_impl, input, span, ui_policy, stderr_is_tty, a2a)
        .await
}
