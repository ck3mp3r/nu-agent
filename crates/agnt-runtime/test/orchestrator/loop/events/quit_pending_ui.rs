//! Quit and idle-drain behavior around the ui-request queue.
//!
//! A switch accepted while the worker looks busy queues, and the queue is
//! drained only from the two arms that end a turn or a blocking request. These
//! tests pin the two ways that leaves an effect stranded: the loop must apply a
//! queued switch once the worker is idle, and quit must not wait for a response
//! it cannot get.

// region:    --- Modules

#[path = "quit_pending_ui/abandon.rs"]
mod abandon;
#[path = "quit_pending_ui/drain.rs"]
mod drain;

// endregion: --- Modules

use super::*;
