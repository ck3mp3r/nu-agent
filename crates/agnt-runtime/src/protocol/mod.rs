//! The runtime contract seams.
//!
//! Traits and their data live here as they lift out of `agnt-core`; core
//! re-exports each one so its own paths stay stable during the migration.

// region:    --- Modules

mod mcp;

pub use mcp::{McpManagement, McpUsabilityState};

// endregion: --- Modules
