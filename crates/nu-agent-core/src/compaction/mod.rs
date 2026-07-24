mod algorithm;
pub(crate) mod helpers;
mod strategy;

pub use algorithm::compact;
#[cfg(test)]
pub(crate) use algorithm::detect_failure_patterns;
pub use strategy::{CompactionOutcome, CompactionParams, CompactionStrategy};

#[cfg(test)]
mod test;
