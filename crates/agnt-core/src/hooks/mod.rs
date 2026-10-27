//! User hook runners.
//!
//! A hook is an observe-only subscriber: it receives lifecycle events from the
//! bus and runs a Nushell script once per event, with a JSON envelope on stdin.
//! Hook results are ignored — the runner publishes nothing.

// region:    --- Modules

mod context;
mod envelope;
mod process;
mod runner;

pub use context::HookAgentContext;
pub use envelope::{ENVELOPE_VERSION, build_envelope};
pub use process::{HookProcess, HookProcessError, NuProcess};
pub use runner::{failure_warning, spawn_agent_hook_runner, spawn_hook_runner};

#[cfg(test)]
#[path = "../../test/hooks/test.rs"]
mod test;

// endregion: --- Modules

// region:    --- Types

/// How long a mode waits for one running hook subprocess before the process
/// exits.
///
/// Hooks are observe-only, so a slow hook never delays exit past this bound.
/// The wait is per runner, so a mode with N runners can wait up to N times this
/// value in the worst case.
pub const HOOK_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

// endregion: --- Types
