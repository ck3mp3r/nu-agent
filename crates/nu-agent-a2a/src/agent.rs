use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::*;

/// Top-level orchestration for running an A2A-compatible agent.
///
/// Starts the HTTP server, registers the mDNS service, creates the A2A
/// client, and generates the [`AgentCard`]. This is the main entry point
/// for running an A2A-compatible agent.
pub struct AgentHandle {
    pub server: A2aServer,
    pub discovery_service: DiscoveryService,
    pub client: A2aClient,
    pub card: AgentCard,
    pub cache: Arc<PeerCache>,
    _browser: Option<DiscoveryBrowser>,
}

impl AgentHandle {
    /// Start the full A2A agent stack.
    ///
    /// A default [`AgentCard`] is built from the provided name, optional
    /// description, and skill set. The server binds to a random loopback
    /// port and the assigned URL is propagated back into the card.
    ///
    /// # Errors
    ///
    /// Returns [`A2aError`] if the server cannot bind or the mDNS service
    /// cannot be registered. If mDNS registration fails after the server
    /// has started, the server is dropped (shutting it down).
    pub async fn start(
        name: &str,
        description: Option<&str>,
        skills: Vec<Skill>,
    ) -> Result<Self, A2aError> {
        match Self::start_inner(name, description, skills).await {
            Ok(handle) => Ok(handle),
            Err((err, Some(server))) => {
                server.shutdown().await;
                Err(err)
            }
            Err((err, None)) => Err(err),
        }
    }

    async fn start_inner(
        name: &str,
        description: Option<&str>,
        skills: Vec<Skill>,
    ) -> Result<Self, (A2aError, Option<A2aServer>)> {
        let mut card = AgentCard {
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            url: "http://127.0.0.1:0".to_string(),
            provider: None,
            icon_url: None,
            documentation_url: None,
            supported_interfaces: vec![AgentInterface {
                url: "http://127.0.0.1:0".into(),
                protocol_version: "1.0".into(),
            }],
            version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: AgentCapabilities::default(),
            skills,
            security_schemes: HashMap::new(),
            extensions: vec![],
            metadata: None,
        };

        let cache = Arc::new(PeerCache::new());

        let server = match A2aServer::start(card.clone(), cache.clone()).await {
            Ok(s) => s,
            Err(e) => return Err((e, None)),
        };
        card.url = server.local_url.clone();
        if let Some(iface) = card.supported_interfaces.first_mut() {
            iface.url = server.local_url.clone();
        }

        let port = server.port;
        let discovery_service = match DiscoveryService::register(name, port, &card) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("mDNS registration failed (non-fatal): {e}");
                DiscoveryService::noop()
            }
        };

        // ── mDNS discovery browser ────────────────────────────────────────
        // Also discover agents on the network.  This is best-effort — failure
        // is non-fatal (local discovery still works).
        let browser = match DiscoveryBrowser::browse() {
            Ok((b, mut rx)) => {
                let cache_clone = cache.clone();
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            PeerEvent::PeerDiscovered(peer) => {
                                cache_clone.add_or_update(*peer);
                            }
                            PeerEvent::PeerLost(name) => {
                                cache_clone.remove(&name);
                            }
                        }
                    }
                });
                Some(b)
            }
            Err(_) => None,
        };

        let client = A2aClient::new();
        Ok(Self {
            server,
            discovery_service,
            client,
            card,
            cache,
            _browser: browser,
        })
    }

    /// Start with a pre-built [`AgentCard`] (e.g., from persona
    /// configuration).
    ///
    /// The card's `url` is updated with the server's assigned address
    /// after the server starts.
    ///
    /// # Errors
    ///
    /// Returns [`A2aError`] if the server cannot bind or the mDNS service
    /// cannot be registered.
    pub async fn start_with_card(card: AgentCard) -> Result<Self, A2aError> {
        match Self::start_with_card_inner(card).await {
            Ok(handle) => Ok(handle),
            Err((err, Some(server))) => {
                server.shutdown().await;
                Err(err)
            }
            Err((err, None)) => Err(err),
        }
    }

    async fn start_with_card_inner(
        mut card: AgentCard,
    ) -> Result<Self, (A2aError, Option<A2aServer>)> {
        let cache = Arc::new(PeerCache::new());

        let server = match A2aServer::start(card.clone(), cache.clone()).await {
            Ok(s) => s,
            Err(e) => return Err((e, None)),
        };
        card.url = server.local_url.clone();
        if let Some(iface) = card.supported_interfaces.first_mut() {
            iface.url = server.local_url.clone();
        }

        let port = server.port;
        let discovery_service = match DiscoveryService::register(&card.name, port, &card) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("mDNS registration failed (non-fatal): {e}");
                DiscoveryService::noop()
            }
        };

        // mDNS browser
        let browser = match DiscoveryBrowser::browse() {
            Ok((b, mut rx)) => {
                let cache_clone = cache.clone();
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            PeerEvent::PeerDiscovered(peer) => {
                                cache_clone.add_or_update(*peer);
                            }
                            PeerEvent::PeerLost(name) => {
                                cache_clone.remove(&name);
                            }
                        }
                    }
                });
                Some(b)
            }
            Err(_) => None,
        };

        let client = A2aClient::new();
        Ok(Self {
            server,
            discovery_service,
            client,
            card,
            cache,
            _browser: browser,
        })
    }

    /// Access the agent's [`TaskStore`].
    pub fn task_store(&self) -> Arc<TaskStore> {
        self.server.task_store()
    }

    /// Gracefully shut down the agent, stopping the server.
    ///
    /// Consumes `self` so it cannot be called twice. The mDNS service is
    /// automatically unregistered when the [`DiscoveryService`] is dropped.
    pub async fn shutdown(self) {
        self.shutdown_with_timeout(Duration::from_secs(5)).await;
    }

    /// Shut down with a custom timeout for graceful shutdown.
    ///
    /// Consumes `self` so it cannot be called twice.
    pub async fn shutdown_with_timeout(self, timeout: Duration) {
        // Graceful HTTP server shutdown.
        tokio::time::timeout(timeout, self.server.shutdown())
            .await
            .ok();

        // DiscoveryService is dropped when this function returns, which
        // sends the mDNS goodbye packet automatically.
    }
}
