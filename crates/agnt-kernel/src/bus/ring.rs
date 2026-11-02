//! The private ring mechanism. Concern types own the public surface; the
//! generic ring stays here.

use std::fmt;

use tokio::sync::broadcast;

/// The subscriber handle of a ring. Seals the transport: consumers never
/// touch the delivery engine's types.
pub struct Subscription<T> {
    rx: broadcast::Receiver<T>,
}

impl<T: Clone> Subscription<T> {
    /// Await the next record. A consumer that stalls past capacity receives
    /// [`ReceiveError::Lagged`] and re-syncs at the tail; older records are
    /// lost exactly as counted.
    pub async fn recv(&mut self) -> Result<T, ReceiveError> {
        match self.rx.recv().await {
            Ok(item) => Ok(item),
            Err(broadcast::error::RecvError::Lagged(count)) => Err(ReceiveError::Lagged(count)),
            Err(broadcast::error::RecvError::Closed) => Err(ReceiveError::Closed),
        }
    }

    /// Non-blocking receive.
    pub fn try_recv(&mut self) -> Result<T, TryReceiveError> {
        match self.rx.try_recv() {
            Ok(item) => Ok(item),
            Err(broadcast::error::TryRecvError::Empty) => Err(TryReceiveError::Empty),
            Err(broadcast::error::TryRecvError::Closed) => Err(TryReceiveError::Closed),
            Err(broadcast::error::TryRecvError::Lagged(count)) => {
                Err(TryReceiveError::Lagged(count))
            }
        }
    }
}

/// Publish failures. The only case: no subscribers — the record was not
/// delivered anywhere and returns to the publisher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishError {
    NoSubscribers,
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSubscribers => {
                write!(f, "publish failed: the ring has no subscribers")
            }
        }
    }
}

impl std::error::Error for PublishError {}

/// Blocking-receive failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveError {
    /// The consumer stalled past capacity; `n` older records were dropped
    /// and the consumer re-syncs at the tail.
    Lagged(u64),
    /// The ring is closed (its owning concern no longer exists).
    Closed,
}

impl fmt::Display for ReceiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lagged(n) => {
                write!(
                    f,
                    "receive lagged: {n} record(s) dropped, re-synced at the tail"
                )
            }
            Self::Closed => write!(f, "receive failed: the ring is closed"),
        }
    }
}

impl std::error::Error for ReceiveError {}

/// Non-blocking receive failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryReceiveError {
    Empty,
    Closed,
    Lagged(u64),
}

impl fmt::Display for TryReceiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "try-receive failed: the ring is empty"),
            Self::Closed => write!(f, "try-receive failed: the ring is closed"),
            Self::Lagged(n) => {
                write!(f, "try-receive lagged: {n} record(s) dropped")
            }
        }
    }
}

impl std::error::Error for TryReceiveError {}

/// Ring-capacity wrapper over a tokio broadcast pair.
#[derive(Clone)]
pub struct Ring<T> {
    tx: broadcast::Sender<T>,
}

impl<T: Clone> Ring<T> {
    /// A ring with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish one item. Returns the subscriber count, or an error when the
    /// ring has no subscribers.
    ///
    /// The kernel does not retain items: a ring is a delivery surface, not a
    /// log; durable history belongs to the session store. An undelivered item
    /// is returned to the publisher ([`PublishError::NoSubscribers`]).
    pub fn publish(&self, item: T) -> Result<usize, PublishError> {
        let count = self
            .tx
            .send(item)
            .map_err(|_| PublishError::NoSubscribers)?;
        Ok(count)
    }

    /// Subscribe a consumer to the ring. Subscriptions see publication order.
    pub fn subscribe(&self) -> Subscription<T> {
        Subscription {
            rx: self.tx.subscribe(),
        }
    }

    /// The number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}
