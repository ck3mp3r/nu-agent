//! The runtime contract seams and the session's event vocabulary.
//!
//! Traits, their data, and the typed ui-event vocabulary live here as they
//! lift out of `agnt-core`.

// region:    --- Modules

mod mcp;

pub mod agents;
pub mod contracts;
pub mod event;
pub mod events;
pub mod model_switching;
pub mod persona;
pub mod picker;
pub mod preamble;
pub mod skills;
pub mod slash;

pub use mcp::{McpManagement, McpUsabilityState};

// endregion: --- Modules

#[cfg(test)]
#[path = "../../test/protocol/events.rs"]
mod events_test;

#[cfg(test)]
#[path = "../../test/protocol/test.rs"]
mod test;
