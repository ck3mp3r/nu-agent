mod login;
mod logout;
mod status;

pub use login::AgentAuthMcpLogin;
pub use logout::AgentAuthMcpLogout;
pub use status::AgentAuthMcpStatus;

#[cfg(test)]
#[path = "login_test.rs"]
mod login_test;

#[cfg(test)]
#[path = "logout_test.rs"]
mod logout_test;

#[cfg(test)]
#[path = "status_test.rs"]
mod status_test;
