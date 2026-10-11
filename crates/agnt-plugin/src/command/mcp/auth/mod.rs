mod login;
mod logout;
mod status;

pub use login::AgentAuthMcpLogin;
pub use logout::AgentAuthMcpLogout;
pub use status::AgentAuthMcpStatus;

#[cfg(test)]
#[path = "../../../../test/command/mcp/auth/login.rs"]
mod login_test;

#[cfg(test)]
#[path = "../../../../test/command/mcp/auth/logout.rs"]
mod logout_test;

#[cfg(test)]
#[path = "../../../../test/command/mcp/auth/status.rs"]
mod status_test;
