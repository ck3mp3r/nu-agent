// Re-export the tool definition functions that moved to core, so existing
// callers in the binary (assemble_tool_definitions, tests) don't break.
pub(crate) use nu_agent_core::conversation::builder::builtin_tool_definitions;

use nu_agent_a2a::a2a_tool_defs;

/// Result of assembling the full tool definition set.
pub(crate) struct ToolAssembly {
    pub(crate) tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) baseline_tool_definitions: Vec<nu_agent_core::types::ToolDefinition>,
    pub(crate) available_agents: Vec<nu_agent_core::protocol::persona::PersonaSummary>,
}

/// Assemble the complete set of tool definitions from all sources.
///
/// All tool groups are always registered unconditionally. The permission system
/// (allow/ask/deny) gates actual use at call time — there is no reason to
/// suppress tool registration based on agent topology.
///
/// Order: closures → builtins → messaging → orchestrator → MCP
pub(crate) fn assemble_tool_definitions(
    closure_registry: &nu_agent_core::tools::closure::ClosureRegistry,
    agents_config: &nu_agent_core::config::AgentsConfig,
    discovered_mcp_tools: &[nu_agent_core::tools::mcp::client::McpToolDefinition],
    cwd: &std::path::Path,
    a2a_enabled: bool,
) -> ToolAssembly {
    let mut tool_definitions: Vec<nu_agent_core::types::ToolDefinition> = closure_registry
        .names()
        .filter_map(|name| {
            let resolved = closure_registry.get(name)?;
            Some(nu_agent_core::tools::closure::closure_to_tool_definition(
                name.clone(),
                &resolved.params,
                None,
            ))
        })
        .collect();

    tool_definitions.extend(builtin_tool_definitions());
    // Old mailbox tools removed in favour of A2A:
    // tool_definitions.extend(messaging_tool_definitions());    // send_message
    // tool_definitions.extend(list_agents_tool_definitions());  // list_agents
    if a2a_enabled {
        tool_definitions.extend(a2a_tool_defs().into_iter().map(|def| {
            nu_agent_core::types::ToolDefinition {
                name: def.name,
                description: def.description,
                parameters: def.parameters,
            }
        }));
    }

    let available_agents = {
        use nu_agent_core::protocol::persona::{FsPersonaResolver, PersonaLister};
        let cwd = cwd.to_path_buf();
        let config_dir = nu_agent_core::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .unwrap_or_default();
        let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
        resolver.list_available()
    };
    // Old orchestrator tools removed in favour of A2A:
    // tool_definitions.extend(orchestrator_tool_definitions(&available_agents));  // spawn_agent, terminate_agent

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
    }
}
