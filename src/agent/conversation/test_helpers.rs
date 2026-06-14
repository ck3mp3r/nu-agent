use crate::types::ToolDefinition;
use crate::{
    agent::protocol::{contracts::ProgressUi, event::UiEvent},
    tools::mcp::{
        client::McpToolDefinition,
        config::{McpServerConfig, McpTransportType},
    },
};

#[derive(Default)]
pub(crate) struct TestProgressUi {
    pub(crate) events: Vec<UiEvent>,
}

impl ProgressUi for TestProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

pub(crate) fn mcp_tool(server: &str, name: &str, raw_name: &str) -> McpToolDefinition {
    McpToolDefinition {
        server: server.to_string(),
        name: name.to_string(),
        raw_name: raw_name.to_string(),
        description: Some(format!("{server}:{raw_name}")),
        parameters: Some(serde_json::json!({"type":"object"})),
    }
}

pub(crate) fn tool_definition_named(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: format!("tool {name}"),
        parameters: serde_json::json!({"type":"object"}),
    }
}

pub(crate) fn mcp_server_config(name: &str, enabled: bool) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        transport: McpTransportType::Http,
        enabled,
        url: Some("http://localhost:7777/mcp".to_string()),
        headers: std::collections::HashMap::new(),
        command: None,
        cwd: None,
        args: Vec::new(),
        env: std::collections::HashMap::new(),
    }
}
