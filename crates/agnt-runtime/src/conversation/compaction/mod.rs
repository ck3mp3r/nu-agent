// region:    --- Modules

pub mod compactor;
pub(crate) mod engine;
pub(crate) mod supersede;
pub(crate) mod undo;

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/compactor.rs"]
mod compactor_test;

// endregion: --- Tests
