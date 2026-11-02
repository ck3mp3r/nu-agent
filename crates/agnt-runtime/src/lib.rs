//! agnt-runtime — the orchestration layer of nu-agent.
//!
//! Owns the shared type layer and, as the strangler progresses, the manager
//! traits, the composition root, the turn executor, the hook chain and the
//! permission machine. It publishes onto `agnt-kernel`'s bus and depends on
//! the kernel; the kernel never depends on it, and stays rig-free.

// region:    --- Modules

pub mod protocol;
pub mod types;

// endregion: --- Modules
