mod login;
mod mcp_login;
mod mcp_logout;
mod mcp_status;

pub use login::AgentAuthLogin;
pub use mcp_login::AgentAuthMcpLogin;
pub use mcp_logout::AgentAuthMcpLogout;
pub use mcp_status::AgentAuthMcpStatus;

#[cfg(test)]
mod login_test;

#[cfg(test)]
#[path = "mcp_logout_test.rs"]
mod mcp_logout_test;

#[cfg(test)]
#[path = "mcp_status_test.rs"]
mod mcp_status_test;

#[cfg(test)]
#[path = "mcp_login_test.rs"]
mod mcp_login_test;
