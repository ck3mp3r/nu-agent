//! Generic helper for polling a one-shot async result from a worker thread.

use std::sync::mpsc;

/// Outcome of polling a pending one-shot result receiver.
pub(crate) enum PollOutcome<T> {
    /// A result was available.
    Ready(Result<T, String>),
    /// No result yet — returns the receiver so the caller can re-park it.
    Pending(mpsc::Receiver<Result<T, String>>),
    /// Sender was dropped before sending — worker disconnected.
    Disconnected,
}

/// Poll a one-shot result receiver without blocking.
pub(crate) fn poll_pending<T>(rx: mpsc::Receiver<Result<T, String>>) -> PollOutcome<T> {
    match rx.try_recv() {
        Ok(result) => PollOutcome::Ready(result),
        Err(mpsc::TryRecvError::Empty) => PollOutcome::Pending(rx),
        Err(mpsc::TryRecvError::Disconnected) => PollOutcome::Disconnected,
    }
}

#[cfg(test)]
#[path = "poll_test.rs"]
mod poll_test;
