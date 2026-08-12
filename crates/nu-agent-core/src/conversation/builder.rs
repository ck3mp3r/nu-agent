use std::path::PathBuf;
use std::sync::Arc;

use nu_protocol::{LabeledError, Span};

use crate::bus::Bus;
use crate::compaction::{CompactionParams, CompactionStrategy};
use crate::config::CompactionConfig;
use crate::protocol::persona::PersonaSummary;
use crate::tools::audit::AuditLogger;
use crate::tools::closure::ClosureRegistry;
use crate::tools::executor::ToolExecutor;
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
            description: "Edit or create files. Use operation.type = 'search_replace' with 'search' and 'replacement' string fields (no line numbers needed). Or use operation.type = 'create' with 'content' field. Requires expected_version from a prior read for existing files.".to_string(),
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
            description: "Apply line-range patch operations with compare-and-swap guard. Lines are 1-indexed: range {start: 5, end: 10} replaces lines 5 through 10 inclusive. The replacement string replaces the entire range.".to_string(),
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
                                        "start": { "type": "integer", "minimum": 1, "description": "First line to replace (1-indexed, inclusive)" },
                                        "end": { "type": "integer", "minimum": 1, "description": "Last line to replace (1-indexed, inclusive)" }
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
        ToolDefinition {
            name: "tmux_session".to_string(),
            description: "Manage tmux sessions. List all sessions, get info about a specific session, create a new session (in a directory), or kill a session. Killing requires force=true.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "info", "create", "kill"] },
                    "session": { "type": "string", "description": "Session name (required for info/kill)" },
                    "name": { "type": "string", "description": "Name for the new session (required for create)" },
                    "directory": { "type": "string", "description": "Starting directory for the new session (optional, for create)" },
                    "force": { "type": "boolean", "description": "Must be true to confirm destruction (required for kill)" }
                },
                "required": ["action"]
            }),
        },
        ToolDefinition {
            name: "tmux_window".to_string(),
            description: "Manage tmux windows within a session. Create a new window (optionally with a name, directory, and target index) or kill a window. Killing requires force=true.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["create", "kill"] },
                    "session": { "type": "string", "description": "Session name" },
                    "name": { "type": "string", "description": "Name for the new window (optional, for create)" },
                    "directory": { "type": "string", "description": "Working directory for the new window (optional, for create)" },
                    "index": { "type": "integer", "description": "Target window index (optional, for create)" },
                    "window": { "type": "string", "description": "Window name or index to kill (required for kill action)" },
                    "force": { "type": "boolean", "description": "Must be true to confirm destruction (required for kill)" }
                },
                "required": ["action", "session"]
            }),
        },
        ToolDefinition {
            name: "tmux_pane".to_string(),
            description: "Control tmux panes within a session. List panes, find a pane by name or context, inspect the running process, capture visible output, send a command, split a pane (horizontally or vertically with a size percentage and optional directory), or kill a pane. Killing requires force=true.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "find", "process", "capture", "send", "split", "kill"] },
                    "session": { "type": "string", "description": "Session name" },
                    "pane": { "type": "string", "description": "Pane ID (optional, for capture/send/split/kill/process)" },
                    "command": { "type": "string", "description": "Command to send to the pane (required for send)" },
                    "direction": { "type": "string", "enum": ["horizontal", "vertical"], "description": "Split direction (optional, for split)" },
                    "size": { "type": "integer", "description": "Size of new pane as percentage (optional, for split)" },
                    "directory": { "type": "string", "description": "Working directory for the new pane (optional, for split)" },
                    "name": { "type": "string", "description": "Pane name to find (optional, for find)" },
                    "context": { "type": "string", "description": "Context to search for, e.g. directory name or command (optional, for find)" },
                    "lines": { "type": "integer", "description": "Number of lines to capture (optional, for capture)" },
                    "force": { "type": "boolean", "description": "Must be true to confirm destruction (required for kill)" }
                },
                "required": ["action", "session"]
            }),
        },
        ToolDefinition {
            name: "tmux_layout".to_string(),
            description: "Select a layout for arranging panes in a tmux window. Non-destructive operation that only changes visual arrangement. Layouts: even-horizontal (equal width columns), even-vertical (equal height rows), main-horizontal (large top pane), main-vertical (large left pane), tiled (grid).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["select"] },
                    "session": { "type": "string", "description": "Session name" },
                    "window": { "type": "string", "description": "Window name or ID" },
                    "layout": { "type": "string", "enum": ["even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled"] }
                },
                "required": ["action", "session", "window", "layout"]
            }),
        },
        ToolDefinition {
            name: "nu".to_string(),
            description: "Execute a Nushell command in a stateless one-shot process. Returns stdout and stderr as text. Use Nushell syntax ONLY (NOT bash/sh/zsh). Each call is independent — no state preserved between calls. Use pipes to chain commands within a single call.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Nushell command to execute. Use Nushell syntax ONLY (NOT bash/sh/zsh). Example: 'ls | where type == file | get name'"
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Timeout in seconds. Default 300."
                    }
                },
                "required": ["command"]
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
    pub tool_server_handle: &'a ToolServerHandle,
    pub closure_registry: &'a ClosureRegistry,
    pub cwd: PathBuf,
    pub engine: &'a EngineInterface,
    /// Span used for error labels (call.head in the binary).
    pub span: Span,
    pub available_agents: &'a [PersonaSummary],
    pub messaging_identity: Option<String>,
    pub tool_timeout: std::time::Duration,
    pub session: Option<&'a mut crate::session::Session>,
    pub max_tool_result_bytes: usize,
    /// Signal bus for tool cancellation and events.
    pub bus: Bus,
    /// Already-merged compaction config (defaults ← plugin config ← CLI flags).
    pub merged_compaction: CompactionConfig,
}

/// Artifacts produced by the builder's `build()` call.
pub struct BuildArtifacts {
    pub parent_name: Option<String>,
    pub merged_compaction: CompactionConfig,
    pub compaction_strategy: CompactionStrategy,
    pub compaction_params: CompactionParams,
}

/// Builder that registers all agent tools and wires multi-agent infrastructure.
///
/// Absorbs the registration logic that was previously in the binary's
/// `register_tools` function, eliminating the layering violation where the
/// binary directly constructed `OrchestratorState` and called `adapt_closures`.
pub struct AgentRuntimeBuilder<'a> {
    input: BuildInput<'a>,
}

impl<'a> AgentRuntimeBuilder<'a> {
    pub fn new(input: BuildInput<'a>) -> Self {
        Self { input }
    }

    /// Register all tools and wire multi-agent infrastructure.
    ///
    /// Returns `BuildArtifacts` containing the parent name,
    /// merged compaction config, and resolved compaction strategy.
    pub async fn build(self) -> Result<BuildArtifacts, LabeledError> {
        use crate::hook::adapter::closure::adapt_closures;

        let BuildInput {
            tool_server_handle,
            closure_registry,
            cwd,
            engine,
            span,
            available_agents: _,
            messaging_identity: _,
            tool_timeout,
            session,
            max_tool_result_bytes,
            bus,
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
            tool_server_handle.add_dynamic_tool(tool).await;
        }

        // All tool groups are always registered. The permission system gates actual use.
        let builtin_defs = builtin_tool_definitions();

        // Build CompactionParams from merged compaction config
        merged_compaction.validate().map_err(|msg| {
            LabeledError::new("Compaction config validation failed").with_label(msg, span)
        })?;
        let compaction_params = build_compaction_params(&merged_compaction);
        let compaction_strategy = compaction_params.compaction_strategy;

        // Apply config to session
        if let Some(session) = session {
            session.set_compaction_config(compaction_params.clone());
        }

        for def in builtin_defs {
            crate::tools::handler::builtin_tool::register_builtin(
                def,
                cwd.clone(),
                max_tool_result_bytes,
                bus.clone(),
                tool_server_handle,
            )
            .await;
        }

        Ok(BuildArtifacts {
            parent_name: None,
            merged_compaction,
            compaction_strategy,
            compaction_params,
        })
    }
}

#[cfg(test)]
#[path = "builder_test.rs"]
mod builder_test;
