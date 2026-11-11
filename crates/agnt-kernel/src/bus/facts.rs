//! The facts concern: latest-wins snapshots of kernel state.
//!
//! Delivery contract: latest-wins and droppable. Every record is a whole
//! value for its field family, so a consumer that lags loses intermediate
//! snapshots and never a state: folding the surviving suffix reaches the
//! latest state for every field. Facts carry state only — no commands, no
//! prompt content, and nothing a caller waits on.

use crate::bus::events::FactRecord;
use crate::bus::ring::{PublishError, Ring, Subscription};

/// The facts ring's capacity.
///
/// Each field family publishes a whole snapshot per change, so a modest ring
/// holds every field's latest value many times over. A consumer that stalls
/// re-syncs at the tail and folds the surviving suffix.
const FACTS_CAPACITY: usize = 256;

/// Latest-wins publication of [`FactRecord`]s.
#[derive(Clone)]
pub struct Facts {
    ring: Ring<FactRecord>,
}

impl Facts {
    /// The facts concern with its standard capacity.
    pub fn new() -> Self {
        Self {
            ring: Ring::with_capacity(FACTS_CAPACITY),
        }
    }

    /// The ring capacity a consumer compares its lag against.
    pub fn capacity() -> usize {
        FACTS_CAPACITY
    }

    /// Publish one snapshot.
    pub fn publish(&self, fact: FactRecord) -> Result<usize, PublishError> {
        self.ring.publish(fact)
    }

    /// Subscribe a consumer. Subscriptions see publication order.
    pub fn subscribe(&self) -> Subscription<FactRecord> {
        self.ring.subscribe()
    }

    /// The number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.ring.subscriber_count()
    }
}

impl Default for Facts {
    fn default() -> Self {
        Self::new()
    }
}
