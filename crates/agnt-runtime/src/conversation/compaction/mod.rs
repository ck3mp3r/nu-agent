// region:    --- Modules

pub(crate) mod cleared;
pub mod compactor;
pub mod config;
pub(crate) mod engine;
pub(crate) mod supersede;
pub(crate) mod undo;

pub use config::*;
pub(crate) use engine::Chunks;

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/compactor.rs"]
mod compactor_test;

// endregion: --- Tests
