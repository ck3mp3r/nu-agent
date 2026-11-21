//! The child-priority policy every tool child is spawned through.
//!
//! A tool child — a `nu` command, an MCP stdio server, a hook process, a tmux
//! call — runs at [`CHILD_NICENESS`], below the agent process. Without the
//! demotion a CPU-heavy child competes with the agent's own threads for OS
//! timeslices: the UI stalls while every render-loop arm still sits inside its
//! budget.
//!
//! The policy is one constant, not a config field. The ruling is that every tool
//! child is demoted; a knob only invites the starvation back.
//!
//! `nice` sets the niceness in the process it execs and then execs the payload in
//! place — same pid, same process-group id. Process-group kills, `child.id()`, and
//! the wrapper's identity are unchanged, and every process the payload spawns
//! inherits the niceness across fork/exec.
//!
//! Degraded mode: when `nice` is not on PATH, children spawn bare and one warning
//! records it. A tool call never fails because the policy could not be applied.
//! The decision is a PATH probe rather than a spawn retry, because a site may
//! hand its `Command` to a library that owns the spawn (rmcp's
//! `TokioChildProcess`) and never see the spawn error.
//!
//! Crate-internal on purpose: every planned site lives in this crate, and a `pub`
//! item no other crate consumes is ARCH-6 surface.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::sync::OnceLock;
use tokio::process::Command;

/// The niceness every tool child runs at.
pub(crate) const CHILD_NICENESS: i32 = 10;

/// The wrapper that sets a niceness before it execs the payload.
const NICE_PROGRAM: &str = "nice";

/// The probe result, read once: one PATH read and at most one warning per process.
static POLICY_ENABLED: OnceLock<bool> = OnceLock::new();

/// A `Command` for a tool child, carrying the child-priority policy.
///
/// The caller chains its own `.arg(..)`, `.current_dir(..)`, `.env(..)`, stdio and
/// `process_group(0)` after this call; the wrapper needs nothing but the program
/// name.
pub(crate) fn child_command(program: &str) -> Command {
    let (wrapped, argv) = wrapped_argv(program, policy_enabled());
    let mut command = Command::new(wrapped.as_ref());
    command.args(argv.iter().map(Cow::as_ref));
    command
}

// region:    --- Support

/// Whether the policy applies in this process, probed once.
fn policy_enabled() -> bool {
    *POLICY_ENABLED.get_or_init(|| {
        if !cfg!(unix) {
            return false;
        }
        let enabled = nice_on_path(std::env::var_os("PATH").unwrap_or_default());
        if !enabled {
            log::warn!(
                "`{NICE_PROGRAM}` is not on PATH: tool children spawn at the agent's own \
priority and the child-priority policy stays degraded"
            );
        }
        enabled
    })
}

/// Whether one PATH value names a `nice` file, so a wrapped spawn will run.
///
/// Empty entries are skipped: an empty PATH element names the working directory
/// in some shells, never this policy's program.
fn nice_on_path(path_value: impl AsRef<OsStr>) -> bool {
    std::env::split_paths(&path_value)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| dir.join(NICE_PROGRAM).is_file())
}

/// The program and leading arguments a child spawn carries.
///
/// When the policy is disabled the program is spawned bare, with no arguments of
/// its own.
fn wrapped_argv<'a>(program: &'a str, enabled: bool) -> (Cow<'a, str>, Vec<Cow<'a, str>>) {
    if !enabled {
        return (Cow::Borrowed(program), Vec::new());
    }
    (
        Cow::Borrowed(NICE_PROGRAM),
        vec![
            Cow::Borrowed("-n"),
            Cow::Owned(CHILD_NICENESS.to_string()),
            Cow::Borrowed(program),
        ],
    )
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/child_priority.rs"]
mod child_priority_test;

// endregion: --- Tests
