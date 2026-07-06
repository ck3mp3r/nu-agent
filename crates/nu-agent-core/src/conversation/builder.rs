use std::path::PathBuf;
use std::sync::Arc;

use nu_protocol::{LabeledError, Span};

use crate::compaction::{CompactionParams, CompactionStrategy};
use crate::config::CompactionConfig;
use crate::mailbox::{AgentMailbox, IncomingMessage, MailboxHandle, socket_dir_for_path};
use crate::protocol::persona::PersonaSummary;
use crate::tools::audit::AuditLogger;
use crate::tools::closure::ClosureRegistry;
use crate::tools::executor::ToolExecutor;
use crate::tools::handler::spawn_agent::OrchestratorState;
use crate::types::ToolDefinition;
use nu_plugin::EngineInterface;
use rig::tool::server::ToolServerHandle;
use serde_json::json;

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
            description: "Edit or create files with explicit mode (preview/apply), CAS guard for existing files, and search_replace/create operations".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "mode": { "type": "string", "enum": ["preview", "apply"], "default": "apply" },
                    "expected_version": { "type": "string", "description": "CAS version from prior read (required for search_replace on existing files)" },
                    "operation": {
                        "type": "object",
                        "properties": {
                            "type": { "type": "string", "enum": ["search_replace", "create"], "default": "search_replace" },
                            "search": { "type": "string", "description": "Required when type is 'search_replace'" },
                            "replacement": { "type": "string", "description": "Required when type is 'search_replace'" },
                            "match_mode": { "type": "string", "enum": ["literal", "regex"], "default": "literal" },
                            "occurrence": { "type": "string", "enum": ["first", "all"], "default": "first" },
                            "content": { "type": "string", "description": "Full file content (required when type is 'create')" }
                        },
                        "required": ["type"]
                    }
                },
                "required": ["path", "operation"]
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
        },        ToolDefinition {
            name: "grep".to_string(),
            description: "Search file contents recursively using a regex pattern. Respects .gitignore. Returns structured matches with file path, line number, and line content.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory or file to search. Defaults to current working directory."
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional file glob filter, e.g. '*.rs' or '*.{ts,tsx}'"
                    },
                    "case_insensitive": {
                        "type": "boolean",
                        "description": "If true, match case-insensitively. Default false."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of matches to return. Default 200."
                    }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "glob".to_string(),
            description: "Find files matching a glob pattern. Respects .gitignore. Returns matching file paths relative to the search root. Use patterns like '**/*.rs' or 'src/**/*.{ts,tsx}'.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern, e.g. '**/*.rs' or 'src/**/*.{ts,tsx}'"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search. Defaults to current working directory."
                    }
                },
                "required": ["pattern"]
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

/// Tool definitions available only to orchestrator agents that can list peers.
pub fn list_agents_tool_definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "list_agents".to_string(),
        description: "List all connected agents and their names".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {},
        }),
    }]
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
    pub mailbox_input: Option<MailboxInput>,
    pub tool_timeout: std::time::Duration,
    pub session: Option<&'a mut crate::session::Session>,
    pub max_tool_result_bytes: usize,
    /// Already-merged compaction config (defaults ← plugin config ← CLI flags).
    pub merged_compaction: CompactionConfig,
}

/// Mailbox parameters for child agents (binary-extracted from CLI flags).
pub struct MailboxInput {
    pub name: String,
    pub parent_name: Option<String>,
}

/// Artifacts produced by the builder's `build()` call.
pub struct BuildArtifacts {
    pub mailbox_rx: Option<std::sync::mpsc::Receiver<IncomingMessage>>,
    pub parent_name: Option<String>,
    pub merged_compaction: CompactionConfig,
    pub compaction_strategy: CompactionStrategy,
    /// Started agent mailbox (socket bound and accept loop spawned).
    /// Dropping it cancels the accept loop and removes the socket file.
    pub mailbox: Option<AgentMailbox>,
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
            mailbox_input,
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

        // All tool groups are always registered. The permission system gates actual use.
        let mut builtin_defs = builtin_tool_definitions();
        builtin_defs.extend(messaging_tool_definitions());
        builtin_defs.extend(list_agents_tool_definitions());
        builtin_defs.extend(orchestrator_tool_definitions(available_agents));

        // Bind the orchestrator mailbox for top-level agents (no parent).
        // An agent with --name but no --parent-name is a top-level orchestrator.
        let is_top_level = mailbox_input
            .as_ref()
            .map(|m| m.parent_name.is_none())
            .unwrap_or(true);

        // Prepare orchestrator socket (sync — no Tokio needed yet).
        // start() is deferred until after all block_on() calls below.
        let (orchestrator_state, orch_handle) = if is_top_level {
            let name = messaging_identity
                .clone()
                .unwrap_or_else(|| "orchestrator".to_string());
            let socket_dir = socket_dir_for_path(&cwd);
            let handle = MailboxHandle::prepare(&socket_dir, &name).map_err(|e| {
                LabeledError::new(format!("Failed to prepare orchestrator mailbox: {e}"))
            })?;
            log::debug!(
                "Orchestrator mailbox prepared: {}",
                handle.socket_path().display()
            );

            let state = Some(Arc::new(std::sync::Mutex::new(OrchestratorState {
                agent_identity: messaging_identity.clone(),
                ..OrchestratorState::new(cwd.clone())
            })));
            (state, Some(handle))
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

        // Prepare child socket only when this agent was spawned by another agent
        // (has a parent-name). Top-level agents use the orchestrator socket above.
        let (child_handle, parent_name) = if let Some(ref input) = mailbox_input {
            if input.parent_name.is_some() {
                let socket_dir = socket_dir_for_path(&cwd);
                let handle = MailboxHandle::prepare(&socket_dir, &input.name).map_err(|e| {
                    LabeledError::new(format!("Failed to prepare agent mailbox: {e}"))
                })?;
                log::debug!(
                    "Agent '{}' mailbox prepared: {}",
                    input.name,
                    handle.socket_path().display()
                );
                (Some(handle), input.parent_name.clone())
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let builtin_tools = adapt_builtins(
            builtin_defs,
            cwd.clone(),
            orchestrator_state,
            socket_dir_for_path(&cwd),
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

        // All block_on() calls are done. Now enter the runtime context so that
        // MailboxHandle::start() can call tokio::spawn. The enter guard lives
        // until the end of build() — long enough to cover both start() calls.
        // block_on() panics if called while an enter guard is active, which is
        // why this guard is placed after all block_on() calls above.
        let _enter = runtime.enter();

        // Start orchestrator mailbox (spawns the accept loop).
        let (orch_mailbox, orch_rx) = match orch_handle {
            Some(handle) => {
                let (mailbox, rx) = handle.start().map_err(|e| {
                    LabeledError::new(format!("Failed to start orchestrator mailbox: {e}"))
                })?;
                (Some(mailbox), Some(rx))
            }
            None => (None, None),
        };

        // Start child mailbox (spawns the accept loop) and pick the right rx.
        let (child_mailbox, mailbox_rx) = match child_handle {
            Some(handle) => {
                let (mailbox, rx) = handle.start().map_err(|e| {
                    LabeledError::new(format!("Failed to start agent mailbox: {e}"))
                })?;
                (Some(mailbox), Some(rx))
            }
            None => (None, orch_rx),
        };

        Ok(BuildArtifacts {
            mailbox_rx,
            parent_name,
            merged_compaction,
            compaction_strategy,
            mailbox: child_mailbox.or(orch_mailbox),
        })
    }
}

#[cfg(test)]
#[path = "builder_test.rs"]
mod builder_test;
