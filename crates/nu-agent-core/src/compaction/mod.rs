mod algorithm;
mod helpers;
mod strategy;

#[cfg(test)]
#[path = "algorithm_test.rs"]
mod algorithm_test;
#[cfg(test)]
#[path = "helpers_test.rs"]
mod helpers_test;
#[cfg(test)]
#[path = "strategy_test.rs"]
mod strategy_test;

pub use algorithm::compact;
#[cfg(test)]
pub(crate) use algorithm::detect_failure_patterns;
pub use strategy::{CompactionOutcome, CompactionParams, CompactionStrategy};
