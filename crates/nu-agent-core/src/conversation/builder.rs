use std::path::PathBuf;
use std::sync::Arc;

use nu_protocol::{LabeledError, Span};

use crate::compaction::{CompactionParams, CompactionStrategy};
use crate::config::CompactionConfig;
use crate::mailbox::{AgentRegistry, BrokerSender, IncomingMessage, ServerFrame};
use crate::protocol::persona::PersonaSummary;
use crate::tools::audit::AuditLogger;
use crate::tools::closure::ClosureRegistry;
use crate::tools::executor::ToolExecutor;
use crate::tools::handler::spawn_agent::OrchestratorState;
use crate::types::ToolDefinition;
use nu_plugin::EngineInterface;
use rig::tool::server::ToolServerHandle;
use serde_json::json;
use tokio::sync::RwLock;

// ── Compaction helpers (moved from binary runtime_build.rs) ─────────────────

/// Merge two `CompactionConfig`s with `override_cfg` taking precedence.
///
/// For each field, if `override_cfg` has `Some`, use it; otherwise keep `base`.
/// Both inputs are `Option<&CompactionConfig>` — `None` means "no config from this source".
pub fn merge_compaction_configs(
    base: Option<&CompactionConfig>,
    override_cfg: &CompactionConfig,
) -> CompactionConfig {
    let base = base.cloned().unwrap_or_default();
    CompactionConfig {
        strategy: override_cfg.strategy.or(base.strategy),
        keep_recent: override_cfg.keep_recent.or(base.keep_recent),
        token_budget: override_cfg.token_budget.or(base.token_budget),
        proactive_threshold_pct: override_cfg
            .proactive_threshold_pct
            .or(base.proactive_threshold_pct),
    }
}

/// Build a `CompactionParams` from a merged `CompactionConfig`.
///
/// Applies `CompactionConfig` field overrides on top of `CompactionParams::default()`.
/// Fields that are `None` in the config use the `CompactionParams` defaults.
pub fn build_compaction_params(merged: &CompactionConfig) -> CompactionParams {
    let mut config = CompactionParams::default();

    if let Some(strategy) = merged.strategy {
        config.compaction_strategy = strategy;
    }
    if let Some(keep_recent) = merged.keep_recent {
        config.keep_recent = keep_recent;
    }
    if let Some(token_budget) = merged.token_budget {
        config.token_budget = Some(token_budget);
    }

    config
}

// ── Tool definitions (moved from binary tool_defs.rs) ───────────────────────

/// Built-in tool definitions for file system and HTTP operations.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read".to_string(),
            description: "Read file content with optional line windowing and return content/version metadata".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "edit".to_string(),
            description: "Canonical edit contract with explicit mode (preview/apply), CAS guard, and legacy compatibility".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "mode": { "type": "string", "enum": ["preview", "apply"], "default": "apply" },
                    "expected_version": { "type": "string" },
                    "operation": {
                        "type": "object",
                        "properties": {
                            "type": { "type": "string", "enum": ["search_replace"], "default": "search_replace" },
                            "search": { "type": "string" },
                            "replacement": { "type": "string" },
                            "match_mode": { "type": "string", "enum": ["literal", "regex"], "default": "literal" },
                            "occurrence": { "type": "string", "enum": ["first", "all"], "default": "first" }
                        },
                        "required": ["search", "replacement"]
                    },
                    "search": { "type": "string", "description": "legacy compatibility field; prefer operation.search" },
                    "replacement": { "type": "string", "description": "legacy compatibility field; prefer operation.replacement" },
                    "match_mode": { "type": "string", "enum": ["literal", "regex"], "description": "legacy compatibility field; prefer operation.match_mode" },
                    "occurrence": { "type": "string", "enum": ["first", "all"], "description": "legacy compatibility field; prefer operation.occurrence" }
                },
                "required": ["path", "expected_version"]
            }),
        },
        ToolDefinition {
            name: "patch".to_string(),
            description: "Apply line-range patch operations with compare-and-swap guard".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "expected_version": { "type": "string" },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "range": {
                                    "type": "object",
                                    "properties": {
                                        "start": { "type": "integer", "minimum": 1 },
                                        "end": { "type": "integer", "minimum": 1 }
                                    },
                                    "required": ["start", "end"]
                                },
                                "replacement": { "type": "string" }
                            },
                            "required": ["range", "replacement"]
                        }
                    }
                },
                "required": ["path", "expected_version", "operations"]
            }),
        },
        ToolDefinition {
            name: "skill".to_string(),
            description: "Load skill content by explicit name from local or home skill roots".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "http".to_string(),
            description: "Fetch content from a URL. Returns markdown extracted from HTML pages, \
                preserving structure (headings, lists, links, code blocks, tables). \
                Raw mode returns the unmodified response body. \
                Respects max_length to avoid context overflow.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["markdown", "raw"],
                        "description": "Response format. markdown (default): converts HTML to markdown. raw: returns body as-is."
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum response length in characters (default: 12000). Responses are truncated if longer."
                    }
                },
                "required": ["url"]
            }),
        },
    ]
}

/// Tool definitions available to any agent that can send messages.
pub fn messaging_tool_definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "send_message".to_string(),
        description: "Send a message to another agent. Messages are delivered as conversation turns to the target agent. \
                       Use the agent names provided in your task instructions — you know your own name and your \
                       --parent-name at birth. The response comes back asynchronously as a new conversation turn.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "to": { "type": "string", "description": "Target agent name" },
                "message": { "type": "string", "description": "Message content" },
                "kind": {
                    "type": "string",
                    "description": "Message type: 'message' (generic/informational, default), 'task' (task assignment), 'completion' (task results), 'question' (blocked, needs decision)",
                    "enum": ["message", "task", "completion", "question"],
                    "default": "message"
                }
            },
            "required": ["to", "message"]
        }),
    }]
}

/// Tool definitions for orchestrator agents that can spawn sub-agents.
pub fn orchestrator_tool_definitions(available_agents: &[PersonaSummary]) -> Vec<ToolDefinition> {
    let description = if available_agents.is_empty() {
        "Spawn a new agent in a tmux pane. No agent personas found. Create .agents/<name>.md files to define agents.".to_string()
    } else {
        let mut desc = String::from(
            "Spawn a new agent in a tmux pane (in a window called \"agents\"). \
             Communicate with spawned agents via `send_message`. \
             The user can also interact directly with spawned agent panes.\n\n\
             Available agents:\n",
        );
        for agent in available_agents {
            desc.push_str(&format!("- {}", agent.name));
            if let Some(ref d) = agent.description {
                desc.push_str(&format!(": {}", d));
            }
            desc.push('\n');
        }
        desc
    };

    vec![
        ToolDefinition {
            name: "spawn_agent".to_string(),
            description,
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": { "type": "string", "description": "Persona name (loads .agents/<name>.md)" },
                    "name": { "type": "string", "description": "Instance identity (optional, defaults to agent-N)" }
                },
                "required": ["agent"]
            }),
        },
        ToolDefinition {
            name: "terminate_agent".to_string(),
            description:
                "Terminate a running sub-agent by name. Kills its tmux pane and deregisters it."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Agent name to terminate" }
                },
                "required": ["name"]
            }),
        },
    ]
}

// ── Builder ──────────────────────────────────────────────────────────────────

/// Input required to build and register agent tools.
///
/// All binary-level extraction (CLI flags, EvaluatedCall parsing) must be
/// completed before constructing this struct. The builder operates entirely on
/// pre-extracted values so that `nu_plugin::EvaluatedCall` never enters core.
pub struct BuildInput<'a> {
    pub runtime: &'a tokio::runtime::Runtime,
    pub tool_server_handle: &'a ToolServerHandle,
    pub closure_registry: &'a ClosureRegistry,
    pub cwd: PathBuf,
    pub engine: &'a EngineInterface,
    /// Span used for error labels (call.head in the binary).
    pub span: Span,
    pub available_agents: &'a [PersonaSummary],
    pub messaging_identity: Option<String>,
    pub broker_flags: Option<BrokerInput>,
    pub is_orchestrator: bool,
    pub has_messaging: bool,
    pub tool_timeout: std::time::Duration,
    pub session: Option<&'a mut crate::session::Session>,
    pub max_tool_result_bytes: usize,
    /// Already-merged compaction config (defaults ← plugin config ← CLI flags).
    pub merged_compaction: CompactionConfig,
}

/// Broker connection parameters (binary-extracted from CLI flags).
pub struct BrokerInput {
    pub socket_path: std::path::PathBuf,
    pub token: String,
    pub parent_name: Option<String>,
}

/// Artifacts produced by the builder's `build()` call.
pub struct BuildArtifacts {
    pub mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
    pub parent_name: Option<String>,
    pub merged_compaction: CompactionConfig,
    pub compaction_strategy: CompactionStrategy,
}

/// Builder that registers all agent tools and wires multi-agent infrastructure.
///
/// Absorbs the registration logic that was previously in the binary's
/// `register_tools` function, eliminating the layering violation where the
/// binary directly constructed `OrchestratorState` and called `adapt_builtins` /
/// `adapt_closures`.
pub struct AgentRuntimeBuilder<'a> {
    input: BuildInput<'a>,
}

impl<'a> AgentRuntimeBuilder<'a> {
    pub fn new(input: BuildInput<'a>) -> Self {
        Self { input }
    }

    /// Register all tools and wire multi-agent infrastructure.
    ///
    /// Returns `BuildArtifacts` containing the mailbox receiver, parent name,
    /// merged compaction config, and resolved compaction strategy.
    pub fn build(self) -> Result<BuildArtifacts, LabeledError> {
        use crate::hook::adapter::builtin::adapt_builtins;
        use crate::hook::adapter::closure::adapt_closures;

        let BuildInput {
            runtime,
            tool_server_handle,
            closure_registry,
            cwd,
            engine,
            span,
            available_agents,
            messaging_identity,
            broker_flags,
            is_orchestrator,
            has_messaging,
            tool_timeout,
            session,
            max_tool_result_bytes,
            merged_compaction,
        } = self.input;

        // Create audit log directory ONCE before prompt loop
        let log_dir = crate::utils::xdg::data_dir()
            .map_err(|e| LabeledError::new(format!("XDG data directory error: {}", e)))?
            .join("nu-agent");
        std::fs::create_dir_all(&log_dir).map_err(|e| {
            LabeledError::new(format!("Failed to create audit log directory: {}", e))
        })?;
        let log_path = log_dir.join("tool_audit.log");

        let audit_logger = Arc::new(AuditLogger::new(log_path));
        let tool_executor = ToolExecutor::new(Arc::new(engine.clone()), audit_logger, tool_timeout);

        // Register closure tools with the ToolServer
        let closure_tools = adapt_closures(
            closure_registry,
            Arc::new(tool_executor.clone()),
            span,
            max_tool_result_bytes,
        );

        for tool in closure_tools {
            runtime
                .block_on(async { tool_server_handle.add_tool(tool).await })
                .map_err(|e| {
                    LabeledError::new(format!("Failed to register closure tool: {}", e))
                        .with_label(format!("{}", e), span)
                })?;
        }

        // Assemble builtin tool definitions
        let mut builtin_defs = builtin_tool_definitions();
        if has_messaging {
            builtin_defs.extend(messaging_tool_definitions());
        }

        // Create orchestrator state for parent agents
        let (orchestrator_state, orchestrator_mailbox_rx) =
            if is_orchestrator {
                builtin_defs.extend(orchestrator_tool_definitions(available_agents));
                let registry = Arc::new(RwLock::new(AgentRegistry::new()));

                let orchestrator_name = messaging_identity
                    .clone()
                    .unwrap_or_else(|| "orchestrator".to_string());
                let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel::<ServerFrame>(64);
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

                let (std_tx, std_rx) = std::sync::mpsc::channel::<IncomingMessage>();
                runtime.spawn(async move {
                while let Some(frame) = tokio_rx.recv().await {
                    if let ServerFrame::Message {
                        from,
                        message,
                        kind,
                    } = frame
                    {
                        log::trace!("Orchestrator received message from '{}': {}", from, message);
                        if std_tx
                            .send(IncomingMessage { from, message, kind })
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

                let state = Some(Arc::new(std::sync::Mutex::new(OrchestratorState {
                    agent_identity: messaging_identity.clone(),
                    ..OrchestratorState::new(registry, cwd.clone())
                })));
                (state, Some(std_rx))
            } else {
                (None, None)
            };

        // Build CompactionParams from merged compaction config
        merged_compaction.validate().map_err(|msg| {
            LabeledError::new("Compaction config validation failed").with_label(msg, span)
        })?;
        let compaction_params = build_compaction_params(&merged_compaction);
        let compaction_strategy = compaction_params.compaction_strategy;

        // Apply config to session
        if let Some(session) = session {
            session.set_compaction_config(compaction_params);
        }

        // Connect to broker if flags provided
        let (broker_sender, mailbox_rx, parent_name) = if let Some(flags) = broker_flags {
            log::debug!("Connecting to broker at {:?}", flags.socket_path);
            let client = runtime
                .block_on(async {
                    crate::mailbox::BrokerClient::connect(&flags.socket_path, &flags.token).await
                })
                .map_err(|e| LabeledError::new(format!("Failed to connect to broker: {}", e)))?;

            log::debug!("Connected to broker as '{}'", client.name);

            let (sender, mut receiver) = client.split();

            let (tx, rx) = std::sync::mpsc::channel();
            runtime.spawn(async move {
                loop {
                    match receiver.recv().await {
                        Ok(ServerFrame::Message {
                            from,
                            message,
                            kind,
                        }) => {
                            log::trace!("Received message from '{}': {}", from, message);
                            if tx
                                .send(IncomingMessage {
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

        let broker_sender_arc =
            broker_sender.map(|s: BrokerSender| Arc::new(tokio::sync::Mutex::new(s)));
        let builtin_tools = adapt_builtins(
            builtin_defs,
            cwd,
            orchestrator_state,
            broker_sender_arc,
            messaging_identity,
            max_tool_result_bytes,
        );

        for tool in builtin_tools {
            runtime
                .block_on(async { tool_server_handle.add_tool(tool).await })
                .map_err(|e| {
                    LabeledError::new(format!("Failed to register builtin tool: {}", e))
                        .with_label(format!("{}", e), span)
                })?;
        }

        Ok(BuildArtifacts {
            mailbox_rx,
            parent_name,
            merged_compaction,
            compaction_strategy,
        })
    }
}

#[cfg(test)]
#[path = "builder_test.rs"]
mod builder_test;
