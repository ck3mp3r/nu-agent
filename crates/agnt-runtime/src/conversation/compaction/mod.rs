// region:    --- Modules

pub(crate) mod availability;
pub mod compactor;
pub(crate) mod engine;
pub(crate) mod summary;
pub(crate) mod supersede;
pub(crate) mod undo;

pub(crate) use availability::{NOTHING_TO_SUMMARIZE, command_availability};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/compactor.rs"]
mod compactor_test;

// endregion: --- Tests
