//! The conversation families the hook chain carries: compaction, the memory
//! state the chain reads, and the token estimate it thresholds on.
//!
//! The rest of the conversation family lands here in slice 8.

// region:    --- Modules

pub mod compaction;
pub mod managers;
pub mod state;
pub mod turn;

// endregion: --- Modules
