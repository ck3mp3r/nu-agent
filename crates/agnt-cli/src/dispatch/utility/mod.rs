//! Utility command dispatch: config, models, provider auth, MCP auth, sessions.

// region:    --- Modules

mod config;
mod mcp;
mod models;
mod provider;
mod session;
mod table;

pub use config::run_config_command;
pub use mcp::run_mcp_command;
pub use models::run_models_command;
pub use provider::run_provider_command;
pub use session::run_session_command;

// endregion: --- Modules
