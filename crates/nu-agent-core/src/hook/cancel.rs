//! Cancellation concern — checks whether the agent turn has been cancelled.

use rig::agent::Flow;
use tokio_util::sync::CancellationToken;

/// Checks a [`CancellationToken`] and returns the appropriate hook actions.
#[derive(Clone)]
pub struct CancelChecker {
    pub token: CancellationToken,
}

impl CancelChecker {
    /// Returns `Some(Flow::terminate(...))` if cancelled, `None` if still running.
    pub fn check_hook(&self) -> Option<Flow> {
        if self.token.is_cancelled() {
            Some(Flow::terminate("Cancelled by user"))
        } else {
            None
        }
    }

    /// Returns `Some(Flow::terminate(...))` if cancelled, `None` if still running.
    pub fn check_tool_call(&self) -> Option<Flow> {
        if self.token.is_cancelled() {
            Some(Flow::terminate("Cancelled by user"))
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "cancel_test.rs"]
mod cancel_test;
