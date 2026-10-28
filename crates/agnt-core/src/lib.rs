pub mod bus;
pub mod compaction;
pub mod config;
pub mod conversation;
pub mod hook;
pub mod hooks;
pub mod llm;
pub mod logging;
pub mod orchestrator;
pub mod protocol;
pub mod renderer;
pub mod session;
pub mod theme_pref;
pub mod tools;
pub mod transcript;
pub mod types;
pub mod utils;

/// The rig-memory crate this workspace's session persistence is built on.
///
/// Re-exported because [`session::SessionManager::rig_store`] and
/// [`session::RigStoreConfig`] hand out its types.
pub use rig_memory;

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;
