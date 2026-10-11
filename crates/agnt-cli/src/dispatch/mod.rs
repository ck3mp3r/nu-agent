//! CLI dispatch: clap args → runtime construction → mode dispatch.

// region:    --- Modules

mod a2a;
mod acp;
mod agent;
mod config;
mod permissions;
mod persona;
mod utility;

pub use a2a::run_a2a;
pub use acp::run_acp;
pub use agent::run_agent;
pub use utility::{
    run_config_command, run_mcp_command, run_models_command, run_provider_command,
    run_session_command,
};

// endregion: --- Modules
