mod strategy;

pub use strategy::{CompactionParams, CompactionStrategy};

#[cfg(test)]
#[path = "../../test/compaction/test.rs"]
mod test;
