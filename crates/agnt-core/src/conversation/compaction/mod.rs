pub mod compactor;
pub mod config;

pub use config::*;

#[cfg(test)]
#[path = "../../../test/conversation/compaction/compactor.rs"]
mod compactor_test;

#[cfg(test)]
#[path = "../../../test/conversation/compaction/policy.rs"]
mod policy_test;
