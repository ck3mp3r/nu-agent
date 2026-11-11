//! The private delivery mechanism. Concern types own the public surface; the
//! generic rings stay here.
//!
//! Two mechanisms, one per delivery contract:
//!
//! - [`Ring`]: a bounded broadcast. Ordered while the consumer keeps up, lossy
//!   under lag (the consumer re-syncs at the tail). The record stream, the
//!   narration tail, and the facts snapshots use it.
//! - [`Strict`]: an unbounded fan-out. Every subscriber attached before a
//!   publish receives it, in publication order, with no ceiling. Control and
//!   lifecycle use it, because a lost command is a command that never ran and
//!   a lost spine step cannot be re-derived.

use std::fmt;
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, mpsc};

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

/// The subscriber handle of a [`Strict`] fan-out.
pub struct StrictSubscription<T> {
    rx: mpsc::UnboundedReceiver<T>,
}

impl<T> StrictSubscription<T> {
    /// Await the next item. Items arrive in publication order and none is
    /// skipped: the queue behind this handle has no ceiling.
    pub async fn recv(&mut self) -> Result<T, ReceiveError> {
        match self.rx.recv().await {
            Some(item) => Ok(item),
            None => Err(ReceiveError::Closed),
        }
    }

    /// Non-blocking receive. A strict handle never reports
    /// [`TryReceiveError::Lagged`].
    pub fn try_recv(&mut self) -> Result<T, TryReceiveError> {
        match self.rx.try_recv() {
            Ok(item) => Ok(item),
            Err(mpsc::error::TryRecvError::Empty) => Err(TryReceiveError::Empty),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(TryReceiveError::Closed),
        }
    }
}

/// Never-dropped fan-out: one unbounded queue per subscriber.
///
/// The broadcast [`Ring`] is lossy by construction — a subscriber that stalls
/// past capacity lags and re-syncs at the tail. Control and lifecycle cannot
/// accept that loss, so they publish here instead: a subscriber registered
/// before a publish receives every later publish in publication order.
///
/// Zero retention, like the ring: an item published with no subscriber
/// attached is returned to the publisher. A subscriber that connects after a
/// publish does not see it — control commands are consumed, not replayed.
pub struct Strict<T> {
    subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<T>>>>,
}

impl<T> Clone for Strict<T> {
    fn clone(&self) -> Self {
        Self {
            subscribers: Arc::clone(&self.subscribers),
        }
    }
}

impl<T> Strict<T> {
    /// An empty fan-out with no subscribers.
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Subscribe a consumer. From here on the consumer observes every publish.
    pub fn subscribe(&self) -> StrictSubscription<T> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.lock().push(tx);
        StrictSubscription { rx }
    }

    /// The number of live subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.lock().len()
    }

    /// The subscriber registry, recovered from a poisoned lock: a panic while
    /// holding it cannot leave a half-written vector.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<mpsc::UnboundedSender<T>>> {
        self.subscribers.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T: Clone> Strict<T> {
    /// Publish one item to every live subscriber. Returns the subscriber
    /// count, or an error when the fan-out has no subscribers.
    pub fn publish(&self, item: T) -> Result<usize, PublishError> {
        let mut subscribers = self.lock();
        if subscribers.is_empty() {
            return Err(PublishError::NoSubscribers);
        }
        let mut delivered = 0usize;
        subscribers.retain(|tx| {
            let live = tx.send(item.clone()).is_ok();
            if live {
                delivered += 1;
            }
            live
        });
        Ok(delivered)
    }
}

impl<T> Default for Strict<T> {
    fn default() -> Self {
        Self::new()
    }
}
