//! Cancellation concern — checks whether the agent turn has been cancelled.

use tokio_util::sync::CancellationToken;

/// Checks a [`CancellationToken`] and returns whether the turn is cancelled.
#[derive(Clone)]
pub struct CancelChecker {
    pub token: CancellationToken,
}

impl CancelChecker {
    /// Returns `true` if the turn has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

#[cfg(test)]
#[path = "cancel_test.rs"]
mod cancel_test;
