//! The lifecycle concern: the ordered, complete turn spine.
//!
//! Delivery contract: ordered and complete. A subscriber attached before a
//! publish observes every later record in publication order, with no capacity
//! ceiling — the attribution map folds the whole sequence, so a skipped step
//! would leave a turn unattributed rather than merely unseen. Zero retention:
//! the spine is consumed forward, never replayed.

use crate::bus::events::LifecycleEvent;
use crate::bus::ring::{PublishError, Strict, StrictSubscription};

/// Ordered, complete publication of [`LifecycleEvent`]s.
#[derive(Clone)]
pub struct Lifecycle {
    strict: Strict<LifecycleEvent>,
}

impl Lifecycle {
    /// The lifecycle concern. Lifecycle has no capacity: the queue behind each
    /// subscriber grows with the records it has not read yet.
    pub fn new() -> Self {
        Self {
            strict: Strict::new(),
        }
    }

    /// Publish one lifecycle record to every live subscriber.
    pub fn publish(&self, event: LifecycleEvent) -> Result<usize, PublishError> {
        self.strict.publish(event)
    }

    /// Subscribe a consumer. From here on it observes every published record.
    pub fn subscribe(&self) -> StrictSubscription<LifecycleEvent> {
        self.strict.subscribe()
    }

    /// The number of live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.strict.subscriber_count()
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}
