//! Conversation state the hook chain carries.

// region:    --- Modules

pub mod mcp;
pub mod memory;
pub mod multi_agent;
pub mod permission;
pub mod persona;
pub mod provider;
pub mod tool;

pub use mcp::McpState;
pub use multi_agent::MultiAgentState;
pub use permission::PermissionState;
pub use persona::{PersonaState, SwitchAgentResult};
pub use provider::ProviderState;
pub use tool::ToolState;

// endregion: --- Modules
