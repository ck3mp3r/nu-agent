use crate::agent::protocol::persona::PersonaSummary;
use rig::completion::ToolDefinition;
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
