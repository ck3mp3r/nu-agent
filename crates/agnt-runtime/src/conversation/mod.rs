//! The conversation families the hook chain carries: compaction, the memory
//! state the chain reads, the token estimate it thresholds on, the runtime the
//! turn executor drives, and the providers it calls.

// region:    --- Modules

pub mod builder;
pub mod compaction;
pub mod contracts;
pub mod managers;
pub mod mcp_helpers;
pub mod mode_logic;
pub mod providers;
pub mod runtime;
pub mod runtime_build;
pub mod runtime_parts;
pub mod state;
pub mod turn;

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/conversation/helpers.rs"]
pub mod test_helpers;

// endregion: --- Tests
