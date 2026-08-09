use std::sync::{Arc, Mutex, RwLock};

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::discovery::PeerDiscoveryImpl;
use crate::*;

pub mod builder;

pub use builder::AgentBuilder;

/// Top-level handle for a running A2A-compatible agent.
///
/// Created by [`builder::AgentBuilder::build`]. The handle provides access
/// to the server, completion events, task store, and graceful shutdown.
pub struct AgentHandle {
    pub server: A2aServer,
    pub completion_tx: Option<mpsc::Sender<A2aCompletionEvent>>,
    completion_rx: Option<mpsc::Receiver<A2aCompletionEvent>>,
    discovery: Arc<Mutex<PeerDiscoveryImpl>>,
    // Private — held for A2aToolContext construction.
    client: A2aClient,
    card: AgentCard,
    /// Handle to the server's mutable AgentCard, for runtime updates
    /// (e.g. after an agent switch). `None` if the server was not
    /// started via `AgentBuilder::build`.
    card_handle: Option<Arc<RwLock<AgentCard>>>,
    cache: Arc<PeerCache>,
    /// The mesh key used for discovery isolation.
    mesh_key: String,
    /// Token to cancel the periodic mDNS re-registration task.
    reregister_token: Option<CancellationToken>,
}

impl AgentHandle {
    /// Access the agent's [`AgentCard`].
    pub fn card(&self) -> &AgentCard {
        &self.card
    }

    /// Access the server's mutable [`AgentCard`] handle for runtime updates.
    ///
    /// Returns `None` if the server was not started via `AgentBuilder::build`
    /// (e.g. in tests that construct `A2aServer` directly).
    pub fn card_handle(&self) -> Option<Arc<RwLock<AgentCard>>> {
        self.card_handle.clone()
    }

    /// Access the peer cache.
    pub fn cache(&self) -> Arc<PeerCache> {
        self.cache.clone()
    }

    /// Access the discovery handle for runtime re-registration.
    pub fn discovery_handle(&self) -> Arc<Mutex<PeerDiscoveryImpl>> {
        self.discovery.clone()
    }

    /// The mesh key used for discovery isolation.
    pub fn mesh_key(&self) -> &str {
        &self.mesh_key
    }

    /// The token used to cancel the periodic mDNS re-registration task.
    pub fn reregister_token(&self) -> Option<&CancellationToken> {
        self.reregister_token.as_ref()
    }

    /// Build an [`A2aToolContext`] for registering A2A tools on the
    /// rig tool server.
    pub fn a2a_tool_context(&self, runtime_handle: tokio::runtime::Handle) -> A2aToolContext {
        A2aToolContext {
            client: self.client.clone(),
            cache: self.cache.clone(),
            own_card: self.card.clone(),
            task_store: Some(self.task_store()),
            completion_tx: self.completion_tx.clone(),
            runtime_handle: Some(runtime_handle),
        }
    }

    /// Access the agent's [`InMemoryTaskStore`].
    pub fn task_store(&self) -> Arc<InMemoryTaskStore> {
        self.server.task_store()
    }

    /// Take the A2A completion event receiver.
    ///
    /// This receiver delivers [`A2aCompletionEvent`] instances when a remote
    /// agent finishes processing a task that was sent via `tasks.send`.
    /// The receiver can only be taken once; subsequent calls return `None`.
    pub fn take_completion_receiver(&mut self) -> Option<mpsc::Receiver<A2aCompletionEvent>> {
        self.completion_rx.take()
    }

    /// Gracefully shut down the agent, stopping the server.
    ///
    /// Consumes `self` so it cannot be called twice.
    pub async fn shutdown(self) {
        self.shutdown_with_timeout(Duration::from_secs(5)).await;
    }

    /// Shut down with a custom timeout for graceful shutdown.
    ///
    /// Consumes `self` so it cannot be called twice.
    pub async fn shutdown_with_timeout(self, timeout: Duration) {
        // Cancel the periodic mDNS re-registration task first, so it stops
        // touching the discovery lock before we shut down discovery.
        if let Some(token) = &self.reregister_token {
            token.cancel();
        }

        // Graceful HTTP server shutdown.
        tokio::time::timeout(timeout, self.server.shutdown())
            .await
            .ok();

        // Shut down discovery (mDNS daemon, etc.).
        // Lock, take what we need, drop the guard, then proceed.
        // Do NOT hold the lock across any .await point.
        let mut discovery = self.discovery.lock().expect("discovery lock");
        discovery.shutdown();
        // Guard dropped here — no .await after this point.
    }
}
