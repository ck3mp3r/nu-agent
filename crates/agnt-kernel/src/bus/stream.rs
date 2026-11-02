//! The record stream concern: the ordered session log.
//!
//! Delivery contract: publish order == receive order; lossy under lag (a
//! consumer that stalls past capacity re-syncs via `Lagged`); zero retention
//! (a ring with no subscribers reports a failed publish; durable history
//! belongs to the session store).

use crate::bus::ring::Ring;
use crate::bus::ring::{PublishError, Subscription};
use crate::records::SessionRecord;

/// Ordered publication of [`SessionRecord`]s.
#[derive(Clone)]
pub struct Stream {
    ring: Ring<SessionRecord>,
}

impl Stream {
    /// A stream with the standard ring capacity (1024).
    pub fn new() -> Self {
        Self {
            ring: Ring::with_capacity(1024),
        }
    }
    /// Publish one record into the ordered stream.
    pub fn publish(&self, record: SessionRecord) -> Result<usize, PublishError> {
        self.ring.publish(record)
    }

    /// Subscribe a consumer. Subscriptions see publication order.
    pub fn subscribe(&self) -> Subscription<SessionRecord> {
        self.ring.subscribe()
    }

    /// The number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.ring.subscriber_count()
    }
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}
