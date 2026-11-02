//! The strict steering concern: cancellation.
//!
//! Delivery contract: subscribe-before-publish — a consumer that must observe
//! cancellation subscribes first and receives the signal with zero added
//! latency; never dropped under flood (the ring is independent of the
//! record stream).

use crate::bus::ring::{PublishError, Ring, Subscription};
use tokio::sync::broadcast;

/// The cancellation vocabulary.
///
/// More variants would require a delivery-contract review; cancellation has
/// none today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelEvent {
    /// The current task was cancelled.
    Requested,
}

/// Strict publication of cancellation signals.
#[derive(Clone)]
pub struct Cancel {
    ring: Ring<CancelEvent>,
}

impl Cancel {
    /// A cancel ring with the standard capacity (64).
    pub fn new() -> Self {
        Self {
            ring: Ring::with_capacity(64),
        }
    }

    /// Request cancellation of the current task.
    pub fn request(&self) -> Result<usize, PublishError> {
        self.ring.publish(CancelEvent::Requested)
    }

    /// Subscribe a consumer. Subscribers that subscribe before the request
    /// receive it; the resolver relies on this ordering.
    pub fn subscribe(&self) -> Subscription<CancelEvent> {
        self.ring.subscribe()
    }
}
