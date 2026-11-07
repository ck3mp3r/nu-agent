//! Authentication error types for MCP server interactions.
//!
//! These errors represent auth-specific failures that should NOT trip the
//! circuit breaker — they are user-actionable (re-login, scope grant) rather
//! than transport-level failures.

use thiserror::Error;

/// Errors that occur during MCP authentication flows.
///
/// Each variant includes a `server` field identifying the MCP server and a
/// user-facing error message with a hint to run `agent mcp auth login`.
#[derive(Debug, Error)]
pub enum McpAuthError {
    /// The server requires authentication but no valid token is available.
    #[error(
        "Authentication required for MCP server '{server}'. \
         Run: agent mcp auth login {server}"
    )]
    AuthRequired { server: String },

    /// The current token lacks the required OAuth scopes.
    #[error(
        "Insufficient scopes for MCP server '{server}'. \
         Required: {required}. \
         Run: agent mcp auth login {server}"
    )]
    InsufficientScope { server: String, required: String },

    /// Token refresh failed (e.g. refresh token expired or revoked).
    #[error(
        "Token refresh failed for MCP server '{server}'. \
         Run: agent mcp auth login {server}"
    )]
    RefreshFailed { server: String },

    /// No OAuth flow has been completed for this server.
    #[error(
        "OAuth flow not completed for MCP server '{server}'. \
         Run: agent mcp auth login {server}"
    )]
    NotAuthenticated { server: String },
}
