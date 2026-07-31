pub mod auth_error;
pub mod circuit_breaker;
pub mod client;
pub mod config;
pub mod credentials;
pub mod namespaced;
pub mod oauth_callback;
pub mod runtime;
pub mod safe_http_client;

pub const MCP_TOOL_NAMESPACE_DELIMITER: &str = "__";
