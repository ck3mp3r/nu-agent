//! The runtime's own state operations: what the surface and the agent ask a
//! connected runtime, and how a runtime merges or loses a server.

use super::McpRuntime;
use super::McpServerLifecycle;
use super::servers::project_server_lifecycle;
use crate::tools::mcp::client::McpToolDefinition;
use crate::tools::mcp::config::McpServerConfig;

impl McpRuntime {
    pub fn has_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn discovered_tools(&self) -> &[McpToolDefinition] {
        &self.discovered_tools
    }

    /// The servers whose connect failed, each with the reason it gave: one
    /// collapsed row per server.
    pub(crate) fn failures(&self) -> &std::collections::BTreeMap<String, String> {
        &self.failures
    }

    pub fn lifecycle_projection(
        &self,
        configured_servers: &[McpServerConfig],
    ) -> Vec<McpServerLifecycle> {
        project_server_lifecycle(configured_servers, &self.connected_servers)
    }

    /// Returns true if a session for `server_name` was established (i.e. the server
    /// was connected at some point and its session is still alive).
    pub fn has_server(&self, server_name: &str) -> bool {
        self.connected_servers.contains(server_name)
    }

    /// Merge a newly-connected runtime into this one.
    ///
    /// Adds the new runtime's sessions, connected-server names, and discovered tools
    /// to this runtime. Used when enabling a previously-unconfigured server.
    /// A server that is up carries no failure: its recorded reason is dropped
    /// here, so the reason never outlives the failure it describes.
    pub fn merge(&mut self, other: McpRuntime) {
        for server_name in &other.connected_servers {
            self.failures.remove(server_name);
        }
        self.sessions.extend(other.sessions);
        self.connected_servers.extend(other.connected_servers);
        self.discovered_tools.extend(other.discovered_tools);
        self.failures.extend(other.failures);
    }

    /// Mark a server as disconnected — removes it from `connected_servers`.
    /// Called when a transport error is detected and the server is being disabled.
    /// The `McpSessionHandle` stays in `sessions` (sessions are one-shot; full
    /// reconnect requires an agent restart). Tool calls will fail until reconnected.
    pub fn mark_disconnected(&mut self, server_name: &str) {
        self.connected_servers.remove(server_name);
        self.failures.remove(server_name);
    }
}
