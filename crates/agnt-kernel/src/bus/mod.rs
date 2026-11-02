//! The two-ring substrate, split by concern.
//!
//! - [`stream::Stream`]: the ordered `SessionRecord` log — the narration and
//!   state-publication pipeline every consumer folds.
//! - [`cancel::Cancel`]: the strict steering ring — subscribe-before-publish,
//!   never-dropped, zero added latency.
//!
//! The generic ring is a private mechanism: the public consumable surface is
//! the concern types themselves, named for behavior.

// region:    --- Modules

mod cancel;
mod ring;
mod stream;

pub use cancel::{Cancel, CancelEvent};
pub use ring::{PublishError, ReceiveError, Subscription, TryReceiveError};
pub use stream::Stream;

// endregion: --- Modules

// region:    --- Types

/// The kernel bus: the composition root of the two concerns.
#[derive(Clone)]
pub struct Bus {
    stream: Stream,
    cancel: Cancel,
}

impl Bus {
    /// A bus with the standard ring capacities: stream 1024, cancel 64.
    pub fn new() -> Self {
        Self {
            stream: Stream::new(),
            cancel: Cancel::new(),
        }
    }

    /// The record stream concern.
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    /// The strict cancel concern.
    pub fn cancel(&self) -> &Cancel {
        &self.cancel
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

// endregion: --- Types
