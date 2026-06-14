use nu_plugin::{EngineInterface, EvaluatedCall};
use nu_protocol::LabeledError;

use crate::tools::closure::ClosureRegistry;

pub(crate) struct SetupResult {
    pub(crate) mailbox_rx:
        Option<std::sync::mpsc::Receiver<crate::agent::mailbox::IncomingMessage>>,
    pub(crate) parent_name: Option<String>,
    pub(crate) merged_compaction: crate::config::CompactionConfig,
    pub(crate) compaction_strategy: crate::compaction::CompactionStrategy,
    pub(crate) compaction_count: usize,
}

pub(crate) struct RegisterToolsInput<'a> {
    pub(crate) runtime: &'a tokio::runtime::Runtime,
    pub(crate) tool_server_handle: &'a rig::tool::server::ToolServerHandle,
    pub(crate) closure_registry: &'a ClosureRegistry,
    pub(crate) engine: &'a EngineInterface,
    pub(crate) call: &'a EvaluatedCall,
    pub(crate) plugin_config_value: Option<&'a nu_protocol::Value>,
    pub(crate) available_agents: &'a [crate::agent::protocol::persona::PersonaSummary],
    pub(crate) messaging_identity: Option<String>,
    pub(crate) broker_flags: Option<super::args::BrokerFlags>,
    pub(crate) is_orchestrator: bool,
    pub(crate) has_messaging: bool,
    pub(crate) tool_timeout: std::time::Duration,
    pub(crate) session: Option<&'a mut crate::session::Session>,
}

/// Register closure tools, builtin tools, and messaging tools with the ToolServer.
/// Connect to broker if broker_flags is Some. Resolve compaction config.
/// Produce orchestrator state and mailbox setup.
pub(crate) fn register_tools(input: RegisterToolsInput<'_>) -> Result<SetupResult, LabeledError> {
    use super::runtime_build::{build_compaction_params, merge_compaction_configs};
    use super::tool_defs::{
        builtin_tool_definitions, messaging_tool_definitions, orchestrator_tool_definitions,
    };
    use crate::agent::hook::builtin_adapter::adapt_builtins;
    use crate::agent::hook::closure_adapter::adapt_closures;
    use crate::config::PluginConfig;

    let RegisterToolsInput {
        runtime,
        tool_server_handle,
        closure_registry,
        engine,
        call,
        plugin_config_value,
        available_agents,
        messaging_identity,
        broker_flags,
        is_orchestrator,
        has_messaging,
        tool_timeout,
        session,
    } = input;

    // Create audit log directory ONCE before prompt loop
    let log_dir = crate::utils::xdg::data_dir()
        .map_err(|e| LabeledError::new(format!("XDG data directory error: {}", e)))?
        .join("nu-agent");
    std::fs::create_dir_all(&log_dir)
        .map_err(|e| LabeledError::new(format!("Failed to create audit log directory: {}", e)))?;
    let log_path = log_dir.join("tool_audit.log");

    let audit_logger = std::sync::Arc::new(crate::tools::audit::AuditLogger::new(log_path));
    let tool_executor = crate::tools::executor::ToolExecutor::new(
        std::sync::Arc::new(engine.clone()),
        audit_logger,
        tool_timeout,
    );

    // Register closure tools with the ToolServer
    // This happens once at startup, not per-turn
    let closure_tools = adapt_closures(
        closure_registry,
        std::sync::Arc::new(tool_executor.clone()),
        call.head,
    );

    for tool in closure_tools {
        runtime
            .block_on(async { tool_server_handle.add_tool(tool).await })
            .map_err(|e| {
                LabeledError::new(format!("Failed to register closure tool: {}", e))
                    .with_label(format!("{}", e), call.head)
            })?;
    }

    // Register builtin FS tools (read, edit, patch, skill) with ToolServer
    let cwd = engine.get_current_dir().map_err(|e| {
        LabeledError::new(format!("Failed to get current directory: {}", e))
            .with_label(format!("{}", e), call.head)
    })?;
    let cwd_path = std::path::PathBuf::from(cwd);

    let mut builtin_defs = builtin_tool_definitions();
    if has_messaging {
        builtin_defs.extend(messaging_tool_definitions());
    }

    // Create orchestrator state for parent agents (no broker_flags = orchestrator)
    // Also register the orchestrator itself in the AgentRegistry so child agents
    // can send messages back to it via send_message(to: "<orchestrator_name>").
    let (orchestrator_state, orchestrator_mailbox_rx) = if is_orchestrator {
        builtin_defs.extend(orchestrator_tool_definitions(available_agents));
        let registry = std::sync::Arc::new(tokio::sync::RwLock::new(
            crate::agent::mailbox::AgentRegistry::new(),
        ));

        // Register the orchestrator in its own registry (use messaging identity)
        let orchestrator_name = messaging_identity
            .clone()
            .unwrap_or_else(|| "orchestrator".to_string());
        let (tokio_tx, mut tokio_rx) =
            tokio::sync::mpsc::channel::<crate::agent::mailbox::ServerFrame>(64);
        runtime.block_on(async {
            registry
                .write()
                .await
                .add_connected(orchestrator_name.clone(), tokio_tx);
        });
        log::debug!(
            "Registered orchestrator '{}' in agent registry",
            orchestrator_name
        );

        // Bridge tokio channel to std::sync::mpsc for mailbox_rx
        let (std_tx, std_rx) = std::sync::mpsc::channel::<crate::agent::mailbox::IncomingMessage>();
        runtime.spawn(async move {
            while let Some(frame) = tokio_rx.recv().await {
                if let crate::agent::mailbox::ServerFrame::Message {
                    from,
                    message,
                    kind,
                } = frame
                {
                    log::trace!("Orchestrator received message from '{}': {}", from, message);
                    if std_tx
                        .send(crate::agent::mailbox::IncomingMessage {
                            from,
                            message,
                            kind,
                        })
                        .is_err()
                    {
                        log::debug!(
                            "Orchestrator mailbox receiver dropped, stopping forwarding task"
                        );
                        break;
                    }
                }
            }
        });

        let state = Some(std::sync::Arc::new(std::sync::Mutex::new(
            crate::agent::tools::handler::spawn_agent::OrchestratorState {
                agent_identity: messaging_identity.clone(),
                ..crate::agent::tools::handler::spawn_agent::OrchestratorState::new(
                    registry,
                    cwd_path.clone(),
                )
            },
        )));
        (state, Some(std_rx))
    } else {
        (None, None)
    };

    // Merge compaction config: default < plugin_config < CLI flags
    // Extract compaction config from plugin config (if available)
    let plugin_compaction = plugin_config_value
        .and_then(|v| PluginConfig::from_plugin_config(v).ok())
        .and_then(|pc| pc.compaction);

    // Extract compaction flags from CLI
    let cli_compaction = super::args::extract_compaction_flags(call)?;

    // Merge: plugin config overrides defaults, CLI overrides plugin config
    let merged_compaction = merge_compaction_configs(plugin_compaction.as_ref(), &cli_compaction);

    // Validate merged compaction config
    merged_compaction.validate().map_err(|msg| {
        LabeledError::new("Compaction config validation failed").with_label(msg, call.head)
    })?;

    // Build CompactionParams from merged compaction config
    let compaction_params = build_compaction_params(&merged_compaction);

    // Extract compaction policy fields (not in CompactionParams)
    let compaction_strategy = compaction_params.compaction_strategy;

    // Apply config to session and extract session metadata
    let compaction_count = if let Some(session) = session {
        session.set_compaction_config(compaction_params);
        session.compaction_count()
    } else {
        0
    };

    // Connect to broker if flags provided
    let (broker_sender, mailbox_rx, parent_name) = if let Some(flags) = broker_flags {
        log::debug!("Connecting to broker at {:?}", flags.socket_path);
        let client = runtime
            .block_on(async {
                crate::agent::mailbox::BrokerClient::connect(&flags.socket_path, &flags.token).await
            })
            .map_err(|e| LabeledError::new(format!("Failed to connect to broker: {}", e)))?;

        log::debug!("Connected to broker as '{}'", client.name);

        let (sender, mut receiver) = client.split();

        // Spawn a task to forward broker messages to a channel
        let (tx, rx) = std::sync::mpsc::channel();
        runtime.spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(crate::agent::mailbox::ServerFrame::Message {
                        from,
                        message,
                        kind,
                    }) => {
                        log::trace!("Received message from '{}': {}", from, message);
                        if tx
                            .send(crate::agent::mailbox::IncomingMessage {
                                from,
                                message,
                                kind,
                            })
                            .is_err()
                        {
                            log::debug!("Mailbox receiver dropped, stopping forwarding task");
                            break;
                        }
                    }
                    Ok(_frame) => {
                        log::trace!("Ignoring non-message frame");
                    }
                    Err(e) => {
                        log::debug!("Broker receiver error: {}", e);
                        break;
                    }
                }
            }
        });

        (Some(sender), Some(rx), flags.parent_name)
    } else {
        (None, orchestrator_mailbox_rx, None)
    };

    let broker_sender_arc = broker_sender.map(|s| std::sync::Arc::new(tokio::sync::Mutex::new(s)));
    let builtin_tools = adapt_builtins(
        builtin_defs,
        cwd_path,
        orchestrator_state,
        broker_sender_arc,
        messaging_identity,
    );

    for tool in builtin_tools {
        runtime
            .block_on(async { tool_server_handle.add_tool(tool).await })
            .map_err(|e| {
                LabeledError::new(format!("Failed to register builtin tool: {}", e))
                    .with_label(format!("{}", e), call.head)
            })?;
    }

    Ok(SetupResult {
        mailbox_rx,
        parent_name,
        merged_compaction,
        compaction_strategy,
        compaction_count,
    })
}
