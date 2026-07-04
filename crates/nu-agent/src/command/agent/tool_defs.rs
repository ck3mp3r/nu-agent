// Re-export the tool definition functions that moved to core, so existing
// callers in the binary (assemble_tool_definitions, tests) don't break.
pub(crate) use nu_agent_core::conversation::builder::{
    builtin_tool_definitions, messaging_tool_definitions, orchestrator_tool_definitions,
};

use nu_agent_core::types::ToolDefinition;
use serde_json::json;

/// Tool definitions available only to orchestrator agents.
pub(crate) fn orchestrator_messaging_tool_definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "list_agents".to_string(),
        description: "List all connected agents and their names".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {},
        }),
    }]
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
    // list_agents requires orchestrator state — only register it for orchestrators
    if is_orchestrator {
        tool_definitions.extend(orchestrator_messaging_tool_definitions());
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
