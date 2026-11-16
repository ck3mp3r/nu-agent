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
pub use runner::{HookSources, failure_warning, spawn_agent_hook_runner, spawn_hook_runner};

#[cfg(test)]
#[path = "../../test/hooks/test.rs"]
mod test;

// endregion: --- Modules

// region:    --- Constants

/// How long one drain waits, in total, for the hook runners to deliver the
/// events already queued when the agent exits.
///
/// One drain shares one deadline across every runner it waits for, so a process
/// with N runners never delays exit past this value.
///
/// The budget has to cover what the queue already holds. A runner serializes its
/// invocations, so at exit the final `agent_shutdown` waits behind the one in
/// flight: one `nu` invocation measures 17-268 ms on a 10-core host under 24
/// concurrent agents, which puts the serialized pair near 540 ms. A bound below
/// that abandons the runner inside its first invocation, and the final envelope
/// is never dispatched at all. This bound covers the pair and still caps exit
/// when a hook hangs.
pub const HOOK_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// How long a mode pauses, with no handle to await, so a session-class hook
/// subprocess already running lands its record before the process exits.
pub const HOOK_EXIT_PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

// endregion: --- Constants
