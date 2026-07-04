//! Cancellation concern — checks whether the agent turn has been cancelled.

use rig::agent::{HookAction, ToolCallHookAction};
use tokio_util::sync::CancellationToken;

/// Checks a [`CancellationToken`] and returns the appropriate hook actions.
#[derive(Clone)]
pub struct CancelChecker {
    pub token: CancellationToken,
}

impl CancelChecker {
    /// Returns `Some(Terminate)` if cancelled, `None` if still running.
    pub fn check_hook(&self) -> Option<HookAction> {
        if self.token.is_cancelled() {
            Some(HookAction::Terminate {
                reason: "Cancelled by user".into(),
            })
        } else {
            None
        }
    }

    /// Returns `Some(Terminate)` if cancelled, `None` if still running.
    pub fn check_tool_call(&self) -> Option<ToolCallHookAction> {
        if self.token.is_cancelled() {
            Some(ToolCallHookAction::Terminate {
                reason: "Cancelled by user".into(),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "cancel_test.rs"]
mod cancel_test;
