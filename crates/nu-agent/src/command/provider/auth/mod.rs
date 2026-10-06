mod login;
mod logout;
mod status;

pub use login::AgentProviderAuthLogin;
pub use logout::AgentProviderAuthLogout;
pub use status::AgentProviderAuthStatus;

#[cfg(test)]
#[path = "../../../../test/command/provider/auth/login.rs"]
mod login_test;

#[cfg(test)]
#[path = "../../../../test/command/provider/auth/logout.rs"]
mod logout_test;

#[cfg(test)]
#[path = "../../../../test/command/provider/auth/status.rs"]
mod status_test;
