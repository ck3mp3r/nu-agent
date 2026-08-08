mod login;
mod logout;
mod status;

pub use login::AgentProviderAuthLogin;
pub use logout::AgentProviderAuthLogout;
pub use status::AgentProviderAuthStatus;

#[cfg(test)]
#[path = "login_test.rs"]
mod login_test;

#[cfg(test)]
#[path = "logout_test.rs"]
mod logout_test;

#[cfg(test)]
#[path = "status_test.rs"]
mod status_test;
