//! Level 1 channel facade — backend-agnostic channel types.
//!
//! This module provides the Level 1 event-base facade: backend-agnostic
//! sender and receiver types that own the application error type and expose
//! only the operations the rest of the codebase needs. The backend is an
//! internal enum (tokio only for now) so the API stays concrete and static —
//! no trait objects. A crossfire backend can be added later as another enum
//! variant without changing the public surface.
//!
//! Level 2 use-case aliases (e.g. [`ToolTx`], [`LlmRx`]) live at the bottom of
//! this module so the topology choice for each channel lives in one place.

use std::future::Future;

use crate::orchestrator::UiStateEvent;

use super::events::{
    CancelEvent, CompactionEvent, ExternalEvent, LlmEvent, PermissionEvent, SessionEvent,
    ToolEvent, TurnEvent, WarningEvent,
};

// region:    --- Types

/// An error produced by a channel operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelError {
    /// The channel was closed (all senders or all receivers dropped).
    #[error("channel closed")]
    Closed,
    /// A broadcast subscriber lagged behind and missed messages.
    #[error("subscriber lagged behind {dropped} events")]
    Lagged { dropped: u64 },
    /// A send failed because no active receiver exists.
    #[error("send failed: no active receiver")]
    NoReceiver,
    /// A send failed because the channel capacity was exceeded.
    #[error("channel capacity exceeded")]
    CapacityExceeded,
}

/// The result of a channel operation.
pub type ChannelResult<T> = Result<T, ChannelError>;

/// An error produced by a non-blocking [`try_recv`](BroadcastRx::try_recv).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryRecvError {
    /// No message is currently available.
    Empty,
    /// The channel was closed.
    Closed,
    /// A broadcast subscriber lagged behind and missed `u64` messages.
    Lagged(u64),
}

/// Optional telemetry counter hook.
///
/// All methods have empty default implementations, so a type like
/// [`NoMetrics`] is monomorphized away and adds zero runtime overhead. The
/// metric name passed to [`increment`](Metrics::increment) is the event kind
/// (`"send"` or `"recv"`), not the channel name — the channel name rides on
/// the [`tracing`](tracing::trace!) events.
pub trait Metrics: Clone + Send + Sync + Default + 'static {
    /// Counts an event on the channel.
    fn increment(&self, _name: &'static str) {}
}

/// Default [`Metrics`] implementation that counts nothing.
///
/// Zero-sized and with only empty method bodies, so a `NoMetrics` field is
/// elided by the compiler and all calls become no-ops.
#[derive(Clone, Default)]
pub struct NoMetrics;

impl Metrics for NoMetrics {}

/// Backend-agnostic broadcast-style sender.
#[derive(Clone)]
pub struct BroadcastTx<M, T: Metrics = NoMetrics> {
    inner: BroadcastBackend<M>,
    name: &'static str,
    metrics: T,
}

/// Backend-agnostic broadcast-style receiver.
///
/// Not `Clone`: the tokio broadcast receiver is not `Clone`, so the facade
/// cannot provide it either.
pub struct BroadcastRx<M, T: Metrics = NoMetrics> {
    inner: BroadcastBackendRx<M>,
    name: &'static str,
    metrics: T,
}

/// Backend-agnostic MPSC sender.
#[derive(Clone)]
pub struct MpscTx<M, T: Metrics = NoMetrics> {
    inner: MpscBackend<M>,
    name: &'static str,
    metrics: T,
}

/// Backend-agnostic MPSC receiver.
pub struct MpscRx<M, T: Metrics = NoMetrics> {
    inner: MpscBackendRx<M>,
    name: &'static str,
    metrics: T,
}

/// Backend-agnostic oneshot sender.
pub struct OneshotTx<M, T: Metrics = NoMetrics> {
    inner: OneshotBackend<M>,
    name: &'static str,
    metrics: T,
}

/// Backend-agnostic oneshot receiver.
///
/// A oneshot receiver is awaited directly (it implements [`Future`]).
pub struct OneshotRx<M, T: Metrics = NoMetrics> {
    inner: OneshotBackendRx<M>,
    name: &'static str,
    metrics: T,
}

#[derive(Clone)]
enum BroadcastBackend<M> {
    Tokio(tokio::sync::broadcast::Sender<M>),
}

enum BroadcastBackendRx<M> {
    Tokio(tokio::sync::broadcast::Receiver<M>),
}

#[derive(Clone)]
enum MpscBackend<M> {
    Tokio(tokio::sync::mpsc::Sender<M>),
}

enum MpscBackendRx<M> {
    Tokio(tokio::sync::mpsc::Receiver<M>),
}

enum OneshotBackend<M> {
    Tokio(tokio::sync::oneshot::Sender<M>),
}

enum OneshotBackendRx<M> {
    Tokio(tokio::sync::oneshot::Receiver<M>),
}

// endregion: --- Types

// region:    --- Broadcast

impl<M: Clone + Send + 'static, T: Metrics> BroadcastTx<M, T> {
    /// Creates a new broadcast channel with the given capacity and a name for
    /// telemetry (e.g. `"tool"`).
    pub fn new(name: &'static str, capacity: usize) -> Self {
        Self::with_metrics(name, capacity, T::default())
    }

    /// Creates a new broadcast channel with a telemetry name, a capacity, and
    /// an explicit metrics hook.
    pub fn with_metrics(name: &'static str, capacity: usize, metrics: T) -> Self {
        Self {
            inner: BroadcastBackend::Tokio(tokio::sync::broadcast::channel(capacity).0),
            name,
            metrics,
        }
    }

    /// Subscribes a new receiver to the channel.
    pub fn subscribe(&self) -> BroadcastRx<M, T> {
        let inner = match &self.inner {
            BroadcastBackend::Tokio(tx) => BroadcastBackendRx::Tokio(tx.subscribe()),
        };
        BroadcastRx {
            inner,
            name: self.name,
            metrics: self.metrics.clone(),
        }
    }

    /// Sends a message to all active receivers.
    pub async fn send(&self, message: M) -> ChannelResult<()> {
        tracing::trace!(channel = self.name, "send");
        self.metrics.increment("send");
        match &self.inner {
            BroadcastBackend::Tokio(tx) => tx
                .send(message)
                .map(|_| ())
                .map_err(|_| ChannelError::NoReceiver),
        }
    }
}

impl<M: Clone + Send + 'static, T: Metrics> BroadcastRx<M, T> {
    /// Receives the next message, awaiting one if none is ready.
    ///
    /// The returned future is select!-compatible and cancellation-safe: each
    /// call creates a fresh future, so dropping it mid-await loses no state.
    pub async fn recv(&mut self) -> ChannelResult<M> {
        tracing::trace!(channel = self.name, "recv");
        self.metrics.increment("recv");
        match &mut self.inner {
            BroadcastBackendRx::Tokio(rx) => rx.recv().await.map_err(|e| match e {
                tokio::sync::broadcast::error::RecvError::Closed => ChannelError::Closed,
                tokio::sync::broadcast::error::RecvError::Lagged(n) => {
                    ChannelError::Lagged { dropped: n }
                }
            }),
        }
    }

    /// Attempts to receive a message without blocking.
    pub fn try_recv(&mut self) -> Result<M, TryRecvError> {
        tracing::trace!(channel = self.name, "recv");
        match &mut self.inner {
            BroadcastBackendRx::Tokio(rx) => rx.try_recv().map_err(|e| match e {
                tokio::sync::broadcast::error::TryRecvError::Empty => TryRecvError::Empty,
                tokio::sync::broadcast::error::TryRecvError::Closed => TryRecvError::Closed,
                tokio::sync::broadcast::error::TryRecvError::Lagged(n) => TryRecvError::Lagged(n),
            }),
        }
    }
}

// endregion: --- Broadcast

// region:    --- Mpsc

impl<M: Send + 'static, T: Metrics> MpscTx<M, T> {
    /// Creates a new MPSC channel with the given capacity and a name for
    /// telemetry.
    pub fn channel(name: &'static str, capacity: usize) -> (MpscTx<M, T>, MpscRx<M, T>) {
        Self::channel_with_metrics(name, capacity, T::default())
    }

    /// Creates a new MPSC channel with a telemetry name, a capacity, and an
    /// explicit metrics hook.
    pub fn channel_with_metrics(
        name: &'static str,
        capacity: usize,
        metrics: T,
    ) -> (MpscTx<M, T>, MpscRx<M, T>) {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        (
            MpscTx {
                inner: MpscBackend::Tokio(tx),
                name,
                metrics: metrics.clone(),
            },
            MpscRx {
                inner: MpscBackendRx::Tokio(rx),
                name,
                metrics,
            },
        )
    }

    /// Sends a message, awaiting capacity if the channel is full.
    pub async fn send(&self, message: M) -> ChannelResult<()> {
        tracing::trace!(channel = self.name, "send");
        self.metrics.increment("send");
        match &self.inner {
            MpscBackend::Tokio(tx) => tx.send(message).await.map_err(|_| ChannelError::Closed),
        }
    }
}

impl<M: Send + 'static, T: Metrics> MpscRx<M, T> {
    /// Receives the next message, awaiting one if none is ready.
    ///
    /// The returned future is select!-compatible and cancellation-safe: each
    /// call creates a fresh future, so dropping it mid-await loses no state.
    pub async fn recv(&mut self) -> ChannelResult<M> {
        tracing::trace!(channel = self.name, "recv");
        self.metrics.increment("recv");
        match &mut self.inner {
            MpscBackendRx::Tokio(rx) => match rx.recv().await {
                Some(m) => Ok(m),
                None => Err(ChannelError::Closed),
            },
        }
    }

    /// Attempts to receive a message without blocking.
    pub fn try_recv(&mut self) -> Result<M, TryRecvError> {
        tracing::trace!(channel = self.name, "recv");
        match &mut self.inner {
            MpscBackendRx::Tokio(rx) => rx.try_recv().map_err(|e| match e {
                tokio::sync::mpsc::error::TryRecvError::Empty => TryRecvError::Empty,
                tokio::sync::mpsc::error::TryRecvError::Disconnected => TryRecvError::Closed,
            }),
        }
    }
}

// endregion: --- Mpsc

// region:    --- Oneshot

impl<M: Send + 'static, T: Metrics> OneshotTx<M, T> {
    /// Creates a new oneshot channel with a name for telemetry.
    pub fn channel(name: &'static str) -> (OneshotTx<M, T>, OneshotRx<M, T>) {
        Self::channel_with_metrics(name, T::default())
    }

    /// Creates a new oneshot channel with a telemetry name and an explicit
    /// metrics hook.
    pub fn channel_with_metrics(
        name: &'static str,
        metrics: T,
    ) -> (OneshotTx<M, T>, OneshotRx<M, T>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (
            OneshotTx {
                inner: OneshotBackend::Tokio(tx),
                name,
                metrics: metrics.clone(),
            },
            OneshotRx {
                inner: OneshotBackendRx::Tokio(rx),
                name,
                metrics,
            },
        )
    }

    /// Sends a message, consuming the sender. Returns the message back if the
    /// receiver was dropped.
    pub fn send(self, message: M) -> ChannelResult<()> {
        tracing::trace!(channel = self.name, "send");
        self.metrics.increment("send");
        match self.inner {
            OneshotBackend::Tokio(tx) => tx.send(message).map_err(|_| ChannelError::Closed),
        }
    }
}

impl<M: Send + 'static, T: Metrics> OneshotRx<M, T> {
    /// Attempts to receive a message without blocking.
    pub fn try_recv(&mut self) -> Result<M, TryRecvError> {
        tracing::trace!(channel = self.name, "recv");
        match &mut self.inner {
            OneshotBackendRx::Tokio(rx) => rx.try_recv().map_err(|e| match e {
                tokio::sync::oneshot::error::TryRecvError::Empty => TryRecvError::Empty,
                tokio::sync::oneshot::error::TryRecvError::Closed => TryRecvError::Closed,
            }),
        }
    }
}

impl<M: Send + 'static, T: Metrics + Unpin> Future for OneshotRx<M, T> {
    type Output = ChannelResult<M>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let res = match &mut self.inner {
            OneshotBackendRx::Tokio(rx) => std::pin::Pin::new(rx).poll(cx),
        };
        match res {
            std::task::Poll::Ready(res) => {
                tracing::trace!(channel = self.name, "recv");
                self.metrics.increment("recv");
                let output = match res {
                    Ok(m) => Ok(m),
                    Err(_) => Err(ChannelError::Closed),
                };
                std::task::Poll::Ready(output)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// endregion: --- Oneshot

// region:    --- Level 2 Aliases

/// Cancel channel.
pub type CancelTx = BroadcastTx<CancelEvent>;
/// Cancel channel receiver.
pub type CancelRx = BroadcastRx<CancelEvent>;
/// Tool lifecycle channel.
pub type ToolTx = BroadcastTx<ToolEvent>;
/// Tool lifecycle channel receiver.
pub type ToolRx = BroadcastRx<ToolEvent>;
/// LLM lifecycle channel.
pub type LlmTx = BroadcastTx<LlmEvent>;
/// LLM lifecycle channel receiver.
pub type LlmRx = BroadcastRx<LlmEvent>;
/// Turn lifecycle channel.
pub type TurnTx = BroadcastTx<TurnEvent>;
/// Turn lifecycle channel receiver.
pub type TurnRx = BroadcastRx<TurnEvent>;
/// Session lifecycle channel.
pub type SessionTx = BroadcastTx<SessionEvent>;
/// Session lifecycle channel receiver.
pub type SessionRx = BroadcastRx<SessionEvent>;
/// External (A2A) channel.
pub type ExternalTx = BroadcastTx<ExternalEvent>;
/// External (A2A) channel receiver.
pub type ExternalRx = BroadcastRx<ExternalEvent>;
/// Compaction lifecycle channel.
pub type CompactionTx = BroadcastTx<CompactionEvent>;
/// Compaction lifecycle channel receiver.
pub type CompactionRx = BroadcastRx<CompactionEvent>;
/// Warning channel.
pub type WarningTx = BroadcastTx<WarningEvent>;
/// Warning channel receiver.
pub type WarningRx = BroadcastRx<WarningEvent>;
/// Permission lifecycle channel.
pub type PermissionTx = BroadcastTx<PermissionEvent>;
/// Permission lifecycle channel receiver.
pub type PermissionRx = BroadcastRx<PermissionEvent>;
/// UI state channel.
pub type UiStateTx = BroadcastTx<UiStateEvent>;
/// UI state channel receiver.
pub type UiStateRx = BroadcastRx<UiStateEvent>;

// endregion: --- Level 2 Aliases
