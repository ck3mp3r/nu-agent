//! The one bridge from the async host to rig's synchronous store contracts.
//!
//! rig 0.45's session API is synchronous: `Session::read`, `Session::save`,
//! `SessionMeta`, and the one-poll `now()` contract are sync methods that are
//! the implementation, not a wrapper over an async twin. Serving them from an
//! async runtime therefore means running them somewhere other than a runtime
//! thread: this module is that one place.
//!
//! A facade closure runs on [`tokio::task::spawn_blocking`], the runtime's own
//! blocking pool. The engine's sync face parks the thread that calls it (its
//! reply crosses a plain channel), so parking a blocking-pool thread is legal
//! and parking a runtime thread never happens. The engine thread itself must
//! never reach the sync face: it is the thread that answers it, so that would
//! wait for a reply only it can produce.

// region:    --- Modules

use std::any::Any;

use thiserror::Error;
use tokio::task::JoinError;

// endregion: --- Modules

// region:    --- Types

/// Why the blocking pool did not answer at all.
///
/// The work's own failure is not here: it travels inside the site's error
/// family, unchanged. This is only what happens when the work never ran to its
/// end.
#[derive(Debug, Error)]
pub enum PoolError {
    /// The work panicked; the payload says what it panicked with.
    #[error("the blocking pool task panicked: {payload}")]
    Panicked {
        /// The panic message, as the panic carried it.
        payload: String,
    },

    /// The work was cancelled before it answered.
    #[error("the blocking pool task was cancelled")]
    Cancelled,
}

// endregion: --- Types

// region:    --- Functions

/// Runs `work` on the blocking pool and answers what it produced.
///
/// The outer error is why the work never ran at all. Whatever the closure
/// answers travels through untouched: a site that answers a `Result` keeps its
/// error family, and a site whose work cannot fail answers the value itself. A
/// panicking closure becomes [`PoolError::Panicked`] carrying its payload, never
/// a silent answer and never a panic in the host task.
pub async fn on_pool<F, T>(work: F) -> Result<T, PoolError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(PoolError::from)
}

// endregion: --- Functions

// region:    --- Froms

impl From<JoinError> for PoolError {
    fn from(error: JoinError) -> Self {
        if error.is_cancelled() {
            return Self::Cancelled;
        }
        Self::Panicked {
            payload: payload_of(error.into_panic()),
        }
    }
}

// endregion: --- Froms

// region:    --- Support

/// What a panic carried: its message when it is text, and a statement that it
/// is not when it is not.
fn payload_of(panic: Box<dyn Any + Send>) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        return (*text).to_owned();
    }
    if let Some(text) = panic.downcast_ref::<String>() {
        return text.clone();
    }
    "a panic payload that is not text".to_owned()
}

// endregion: --- Support
