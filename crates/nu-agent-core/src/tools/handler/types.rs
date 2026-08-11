use nu_plugin::EngineInterface;
use nu_protocol::Span;
use serde_json::Value as JsonValue;

use crate::protocol::event::ToolDisplayStats;
use crate::tools::authz::{
    AskApprovalHook, PermissionEventSink, PermissionsConfig, SessionGrantCache,
};
use crate::tools::{closure::ClosureRegistry, executor::ToolExecutor};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Closure,
    Mcp,
    /// Agent-coordination and read-only builtin tools (`read`, `skill`, `spawn_agent`,
    /// `send_message`, `list_agents`). These bypass the permission system entirely.
    Builtin,
    /// Privileged builtin tools (edit, patch, tmux, nu). Despite being built-in,
    /// these go through the full permission flow because they can modify state
    /// outside the agent.
    BuiltinFs,
    Unknown,
}

impl ToolSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
            Self::Builtin => "builtin",
            Self::BuiltinFs => "builtin_fs",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolErrorKind {
    Timeout,
    Validation,
    Authorization,
    Runtime,
    Transport,
    Unknown,
}

impl ToolErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Validation => "validation",
            Self::Authorization => "authorization",
            Self::Runtime => "runtime",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolHandlerError {
    pub kind: ToolErrorKind,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl ToolHandlerError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: ToolErrorKind::Validation,
            message: message.into(),
            details: None,
        }
    }
    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: ToolErrorKind::Runtime,
            message: message.into(),
            details: None,
        }
    }
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ToolFailureOutcome {
    pub tool_name: String,
    pub tool_call_id: String,
    pub source: ToolSource,
    pub error_kind: ToolErrorKind,
    pub message: String,
    pub details: Option<JsonValue>,
}

#[derive(Debug, Clone)]
pub struct EditPreviewDisplayPayload {
    pub path: String,
    pub diff: String,
    pub stats: ToolDisplayStats,
}

#[derive(Debug, Clone)]
pub struct McpToolRegistry {
    names: std::collections::HashSet<String>,
    raw_name_by_exposed_name: std::collections::HashMap<String, String>,
    server_by_exposed_name: std::collections::HashMap<String, String>,
    enabled_servers: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
}

impl McpToolRegistry {
    pub fn empty() -> Self {
        Self {
            names: std::collections::HashSet::new(),
            raw_name_by_exposed_name: std::collections::HashMap::new(),
            server_by_exposed_name: std::collections::HashMap::new(),
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
        }
    }

    pub fn from_tools<I>(tools: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = crate::tools::mcp::client::McpToolDefinition>,
    {
        let mut names = std::collections::HashSet::new();
        let mut raw_name_by_exposed_name = std::collections::HashMap::new();
        let mut server_by_exposed_name = std::collections::HashMap::new();
        let mut enabled_servers = std::collections::HashSet::new();

        for tool in tools {
            let exposed_name = tool.name;
            let raw_name = tool.raw_name;
            let server_name = tool.server;
            if !names.insert(exposed_name.clone()) {
                return Err(format!(
                    "duplicate exposed MCP tool name '{}' while building MCP registry",
                    exposed_name
                ));
            }
            raw_name_by_exposed_name.insert(exposed_name.clone(), raw_name.clone());
            server_by_exposed_name.insert(exposed_name, server_name.clone());
            enabled_servers.insert(server_name);
        }

        Ok(Self {
            names,
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
        })
    }

    pub fn register_tools<I>(&mut self, tools: I) -> Result<(), String>
    where
        I: IntoIterator<Item = crate::tools::mcp::client::McpToolDefinition>,
    {
        #[derive(Debug)]
        struct PendingTool {
            exposed_name: String,
            raw_name: String,
            server_name: String,
            is_new_mapping: bool,
        }

        let mut enabled_servers = self
            .enabled_servers
            .write()
            .map_err(|_| "MCP enabled-server state lock poisoned".to_string())?;

        let mut pending_tools: Vec<PendingTool> = Vec::new();
        let mut incoming_seen: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut pending_new_mappings: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for tool in tools {
            let exposed_name = tool.name;
            let raw_name = tool.raw_name;
            let server_name = tool.server;
            let is_new_mapping = !self.raw_name_by_exposed_name.contains_key(&exposed_name)
                && pending_new_mappings.insert(exposed_name.clone());

            if let Some((seen_raw, seen_server)) = incoming_seen.get(&exposed_name) {
                if seen_raw != &raw_name {
                    return Err(format!(
                        "conflicting raw MCP tool mapping for '{}': existing='{}' new='{}'",
                        exposed_name, seen_raw, raw_name
                    ));
                }

                if seen_server != &server_name {
                    return Err(format!(
                        "conflicting MCP tool owner for '{}': existing='{}' new='{}'",
                        exposed_name, seen_server, server_name
                    ));
                }
            } else {
                incoming_seen.insert(
                    exposed_name.clone(),
                    (raw_name.clone(), server_name.clone()),
                );
            }

            if let Some(existing_raw) = self.raw_name_by_exposed_name.get(&exposed_name) {
                if existing_raw != &raw_name {
                    return Err(format!(
                        "conflicting raw MCP tool mapping for '{}': existing='{}' new='{}'",
                        exposed_name, existing_raw, raw_name
                    ));
                }

                if let Some(existing_server) = self.server_by_exposed_name.get(&exposed_name)
                    && existing_server != &server_name
                {
                    return Err(format!(
                        "conflicting MCP tool owner for '{}': existing='{}' new='{}'",
                        exposed_name, existing_server, server_name
                    ));
                }
            }

            pending_tools.push(PendingTool {
                exposed_name,
                raw_name,
                server_name,
                is_new_mapping,
            });
        }

        let new_count = pending_tools.iter().filter(|p| p.is_new_mapping).count();
        log::debug!(
            "McpToolRegistry.register_tools: count={} new={new_count}",
            pending_tools.len()
        );

        for pending in pending_tools {
            if pending.is_new_mapping {
                self.names.insert(pending.exposed_name.clone());
                self.raw_name_by_exposed_name
                    .insert(pending.exposed_name.clone(), pending.raw_name.clone());
                self.server_by_exposed_name
                    .insert(pending.exposed_name.clone(), pending.server_name.clone());
            }

            enabled_servers.insert(pending.server_name);
        }

        Ok(())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name) && self.is_tool_enabled(name)
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn raw_name_for(&self, exposed_name: &str) -> Option<&str> {
        self.raw_name_by_exposed_name
            .get(exposed_name)
            .map(String::as_str)
    }

    pub fn server_name_for(&self, exposed_name: &str) -> Option<&str> {
        self.server_by_exposed_name
            .get(exposed_name)
            .map(String::as_str)
    }

    pub fn is_tool_enabled(&self, exposed_name: &str) -> bool {
        let Some(server_name) = self.server_by_exposed_name.get(exposed_name) else {
            return false;
        };

        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }

    pub fn set_server_enabled(&self, server_name: &str, enabled: bool) -> Result<(), String> {
        log::debug!("McpToolRegistry.set_server_enabled: server={server_name} enabled={enabled}");
        let mut servers = self
            .enabled_servers
            .write()
            .map_err(|_| "MCP enabled-server state lock poisoned".to_string())?;

        if enabled {
            servers.insert(server_name.to_string());
        } else {
            servers.remove(server_name);
        }

        Ok(())
    }

    pub fn is_server_enabled(&self, server_name: &str) -> bool {
        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }
}

pub struct ToolAuthorizationContext<'a, H: AskApprovalHook, S: PermissionEventSink> {
    pub permissions: &'a PermissionsConfig,
    pub grant_cache: &'a mut SessionGrantCache,
    pub ask_hook: &'a mut H,
    pub event_sink: &'a mut S,
}

pub struct ToolHandlerContext<'a, H: AskApprovalHook, S: PermissionEventSink> {
    pub closure_registry: &'a ClosureRegistry,
    pub mcp_registry: &'a McpToolRegistry,
    pub mcp_tool_server: Option<&'a rig::tool::server::ToolServerHandle>,
    pub tool_executor: &'a ToolExecutor,
    pub engine: &'a EngineInterface,
    pub authorization: ToolAuthorizationContext<'a, H, S>,
    pub span: Span,
}
