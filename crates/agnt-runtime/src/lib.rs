//! agnt-runtime — the orchestration layer of nu-agent.
//!
//! Owns the shared type layer and, as the strangler progresses, the manager
//! traits, the composition root, the turn executor, the hook chain and the
//! permission machine. It publishes onto `agnt-kernel`'s bus and depends on
//! the kernel; the kernel never depends on it, and stays rig-free.

// region:    --- Modules

pub mod bus;
pub mod config;
pub mod conversation;
pub mod hook;
pub mod hooks;
pub mod llm;
pub mod logging;
pub mod orchestrator;
pub mod protocol;
pub mod session;
pub mod theme_pref;
pub mod tools;
pub mod types;
pub mod utils;

/// The rig-memory crate this workspace's session persistence is built on.
///
/// Re-exported because [`session::RigStoreConfig`] and [`session::SessionMeta`]
/// hand out its types.
pub use rig_memory;

#[cfg(test)]
#[path = "../test/architecture/test.rs"]
mod architecture_test;

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;

// endregion: --- Modules
