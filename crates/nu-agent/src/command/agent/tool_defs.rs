use nu_agent_core::protocol::persona::PersonaSummary;
use nu_agent_core::types::ToolDefinition;
use serde_json::json;

pub(crate) fn builtin_tool_definitions() -> Vec<ToolDefinition> {
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
            parameters: serde_json::json!({
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

pub(crate) fn orchestrator_tool_definitions(
    available_agents: &[PersonaSummary],
) -> Vec<ToolDefinition> {
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

pub(crate) fn messaging_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "send_message".to_string(),
            description: "Send a message to another agent. Messages are delivered as conversation turns to the target agent. \
                           The target agent name must match a spawned agent's --name (use list_agents to discover running agents). \
                           The response comes back asynchronously as a new conversation turn.".to_string(),
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
        },
        ToolDefinition {
            name: "list_agents".to_string(),
            description: "List all connected agents and their names".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}

/// Result of assembling the full tool definition set.
pub(crate) struct ToolAssembly {
    pub(crate) tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) baseline_tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) available_agents: Vec<nu_agent_core::protocol::persona::PersonaSummary>,
    pub(crate) is_orchestrator: bool,
    pub(crate) has_messaging: bool,
}

/// Assemble the complete set of tool definitions from all sources.
///
/// Order: closures → builtins → messaging (if applicable) → orchestrator (if applicable) → MCP
pub(crate) fn assemble_tool_definitions(
    closure_registry: &nu_agent_core::tools::closure::ClosureRegistry,
    has_broker: bool,
    agents_config: &nu_agent_core::config::AgentsConfig,
    discovered_mcp_tools: &[nu_agent_core::tools::mcp::client::McpToolDefinition],
    cwd: &std::path::Path,
) -> ToolAssembly {
    let mut tool_definitions: Vec<nu_agent_core::types::ToolDefinition> = closure_registry
        .names()
        .map(|name| {
            let resolved = closure_registry.get(name).unwrap();
            nu_agent_core::tools::closure::closure_to_tool_definition(
                name.clone(),
                &resolved.params,
                None,
            )
        })
        .collect();

    tool_definitions.extend(builtin_tool_definitions());

    // Only add orchestrator tools (spawn_agent) for parent agents (no broker_flags)
    let is_orchestrator = !has_broker;

    // Add messaging tools when agent has broker access (child) or is orchestrator (parent)
    let has_messaging = has_broker || is_orchestrator;
    if has_messaging {
        tool_definitions.extend(messaging_tool_definitions());
    }
    let available_agents = if is_orchestrator {
        use nu_agent_core::protocol::persona::{FsPersonaResolver, PersonaLister};
        let cwd = cwd.to_path_buf();
        let config_dir = nu_agent_core::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .unwrap_or_default();
        let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
        resolver.list_available()
    } else {
        Vec::new()
    };
    if is_orchestrator {
        tool_definitions.extend(orchestrator_tool_definitions(&available_agents));
    }

    tool_definitions.extend(discovered_mcp_tools.iter().map(|tool| {
        nu_agent_core::types::ToolDefinition {
            name: tool.name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| format!("MCP tool from server '{}'", tool.server)),
            parameters: tool.parameters.clone().unwrap_or_else(|| {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "args": {
                            "type": "array",
                            "items": {}
                        }
                    },
                    "required": ["args"]
                })
            }),
        }
    }));

    // Store baseline for agent switching
    let baseline_tool_definitions = tool_definitions.clone();

    ToolAssembly {
        tool_definitions,
        baseline_tool_definitions,
        available_agents,
        is_orchestrator,
        has_messaging,
    }
}
