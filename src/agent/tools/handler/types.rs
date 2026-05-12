use nu_plugin::EngineInterface;
use nu_protocol::Span;
use rig::completion::message::ToolCall;
use serde_json::Value as JsonValue;

use crate::agent::protocol::event::{ToolDisplay, ToolDisplayStats};
use crate::agent::tools::authz::{
    AskApprovalHook, PermissionEventSink, PermissionsConfig, SessionGrantCache,
};
use crate::tools::{closure::ClosureRegistry, executor::ToolExecutor};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Closure,
    Mcp,
    Unknown,
}

impl ToolSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
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
pub struct ToolFailureOutcome {
    pub tool_name: String,
    pub tool_call_id: String,
    pub source: ToolSource,
    pub error_kind: ToolErrorKind,
    pub message: String,
    pub details: Option<JsonValue>,
}

impl ToolFailureOutcome {
    pub fn to_json_value(&self) -> JsonValue {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "tool_name".to_string(),
            JsonValue::String(self.tool_name.clone()),
        );
        obj.insert(
            "tool_call_id".to_string(),
            JsonValue::String(self.tool_call_id.clone()),
        );
        obj.insert(
            "source".to_string(),
            JsonValue::String(self.source.as_str().to_string()),
        );
        obj.insert(
            "error_kind".to_string(),
            JsonValue::String(self.error_kind.as_str().to_string()),
        );
        obj.insert(
            "message".to_string(),
            JsonValue::String(self.message.clone()),
        );

        if let Some(details) = &self.details {
            obj.insert("details".to_string(), details.clone());
        }

        JsonValue::Object(obj)
    }

    pub(crate) fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_json_value()).unwrap_or_else(|_| {
            format!(
                r#"{{"tool_name":"{}","tool_call_id":"{}","source":"{}","error_kind":"{}","message":"{}"}}"#,
                self.tool_name,
                self.tool_call_id,
                self.source.as_str(),
                self.error_kind.as_str(),
                self.message
            )
        })
    }
}

/// Result of executing a single tool call.
///
/// Contains the tool call ID and the serialized JSON result.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub source: ToolSource,
    pub content: String,
    pub display: Option<ToolDisplay>,
    pub failure: Option<ToolFailureOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationDeniedDetails {
    pub rule_identity: String,
    pub scope: String,
    pub target_field: Option<String>,
    pub pattern: String,
    pub diagnostics: Vec<AuthorizationDiagnostic>,
}

impl AuthorizationDeniedDetails {
    pub fn to_json_value(&self) -> JsonValue {
        let mut details = serde_json::Map::new();
        details.insert(
            "rule_identity".to_string(),
            JsonValue::String(self.rule_identity.clone()),
        );
        details.insert("scope".to_string(), JsonValue::String(self.scope.clone()));
        if let Some(field) = &self.target_field {
            details.insert("target_field".to_string(), JsonValue::String(field.clone()));
        }
        details.insert(
            "pattern".to_string(),
            JsonValue::String(self.pattern.clone()),
        );
        details.insert(
            "diagnostics".to_string(),
            JsonValue::Array(
                self.diagnostics
                    .iter()
                    .map(|diagnostic| {
                        serde_json::json!({
                            "code": diagnostic.code,
                            "message": diagnostic.message,
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
        );

        JsonValue::Object(details)
    }
}

#[derive(Debug, Clone)]
pub struct EditPreviewDisplayPayload {
    pub path: String,
    pub diff: String,
    pub stats: ToolDisplayStats,
}

pub(crate) fn serialized_tool_call_arguments(tool_call: &ToolCall) -> String {
    serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Debug, Clone)]
pub struct McpToolRegistry {
    names: std::collections::HashSet<String>,
    raw_name_by_exposed_name: std::collections::HashMap<String, String>,
    server_by_exposed_name: std::collections::HashMap<String, String>,
    enabled_servers: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
}

impl McpToolRegistry {
    #[cfg(test)]
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names: std::collections::HashSet<String> = names.into_iter().map(Into::into).collect();
        let raw_name_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let server_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let enabled_servers = names.iter().cloned().collect();
        Self {
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
            names,
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
            raw_name_by_exposed_name.insert(exposed_name.clone(), raw_name);
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
