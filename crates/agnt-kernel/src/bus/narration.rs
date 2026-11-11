//! The narration concern: text for a human to read.
//!
//! Delivery contract: order-critical with an oldest-droppable tail. A consumer
//! that falls behind capacity receives `Lagged` and re-syncs at the tail; it
//! then continues in receipt order. The skipped records are the oldest ones —
//! exactly the ones a reader has already stopped caring about — and chunk
//! records carry the aggregated text, so a skip costs no content.

use crate::bus::events::NarrationEvent;
use crate::bus::ring::{PublishError, Ring, Subscription};

/// The narration ring's capacity.
///
/// Sized for a burst of streamed deltas: a consumer that stalls for a whole
/// response re-syncs at its tail and loses only intermediate deltas.
const NARRATION_CAPACITY: usize = 1024;

/// Bounded, order-preserving publication of [`NarrationEvent`]s.
#[derive(Clone)]
pub struct Narration {
    ring: Ring<NarrationEvent>,
}

impl Narration {
    /// The narration concern with its standard capacity.
    pub fn new() -> Self {
        Self {
            ring: Ring::with_capacity(NARRATION_CAPACITY),
        }
    }

    /// The ring capacity a consumer compares its lag against.
    pub fn capacity() -> usize {
        NARRATION_CAPACITY
    }

    /// Publish one narration record.
    pub fn publish(&self, event: NarrationEvent) -> Result<usize, PublishError> {
        self.ring.publish(event)
    }

    /// Subscribe a consumer. Subscriptions see publication order.
    pub fn subscribe(&self) -> Subscription<NarrationEvent> {
        self.ring.subscribe()
    }

    /// The number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.ring.subscriber_count()
    }
}

impl Default for Narration {
    fn default() -> Self {
        Self::new()
    }
}
