//! The record stream concern: the ordered session log.
//!
//! Two lanes, one per delivery contract:
//!
//! - the **event lane** ([`Stream::publish`] / [`Stream::subscribe`]) carries the
//!   discrete records: tool calls, permission steps, notices, compaction,
//!   prompts, session boundaries, turn boundaries, and an inference's start,
//!   completion, and stop. One record of these can never be re-derived, so the
//!   lane exists to keep them.
//! - the **text lane** ([`Stream::publish_text`] / [`Stream::subscribe_text`])
//!   carries the per-delta text of an implementation or a summary: the records a
//!   streaming producer emits once per token. They are high-volume and
//!   self-healing — every one carries the whole text so far — so they ride their
//!   own bounded ring and a flood of them cannot displace an event.
//!
//! Delivery contract, both lanes: publish order == receive order; lossy under lag
//! (a consumer that stalls past capacity re-syncs via `Lagged`); zero retention (a
//! ring with no subscribers reports a failed publish; durable history belongs to
//! the session store).

use crate::bus::ring::Ring;
use crate::bus::ring::{PublishError, Subscription};
use crate::records::SessionRecord;

/// Ordered publication of [`SessionRecord`]s on two lanes: the event lane and
/// the text lane.
#[derive(Clone)]
pub struct Stream {
    ring: Ring<SessionRecord>,
    text: Ring<SessionRecord>,
}

impl Stream {
    /// A stream with the standard ring capacity (1024) on each lane.
    pub fn new() -> Self {
        Self {
            ring: Ring::with_capacity(1024),
            text: Ring::with_capacity(1024),
        }
    }
    /// Publish one discrete record into the event lane.
    pub fn publish(&self, record: SessionRecord) -> Result<usize, PublishError> {
        self.ring.publish(record)
    }

    /// Subscribe a consumer to the event lane. Subscriptions see publication
    /// order.
    pub fn subscribe(&self) -> Subscription<SessionRecord> {
        self.ring.subscribe()
    }

    /// Publish one per-delta text record into the text lane.
    ///
    /// A record published here never displaces an event: the two lanes hold
    /// their own capacity, so an implementation's stream of deltas leaves the
    /// event ring as free as it was before the stream started.
    pub fn publish_text(&self, record: SessionRecord) -> Result<usize, PublishError> {
        self.text.publish(record)
    }

    /// Subscribe a consumer to the text lane. Subscriptions see publication
    /// order, and a consumer that stalls past capacity re-syncs at the tail:
    /// every text record carries the whole text so far, so a skipped delta costs
    /// no content.
    pub fn subscribe_text(&self) -> Subscription<SessionRecord> {
        self.text.subscribe()
    }

    /// The number of active subscribers on the event lane.
    pub fn subscriber_count(&self) -> usize {
        self.ring.subscriber_count()
    }
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}
