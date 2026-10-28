use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::AgentHandle;
use crate::discovery::{PeerDiscoveryImpl, PeerEvent};
use crate::*;

/// Builder for creating and starting an A2A-compatible agent.
///
/// Configure the agent with a name, optional description, skills, port,
/// and mesh key, then call [`build`](AgentBuilder::build) to start the
/// server and return an [`AgentHandle`].
pub struct AgentBuilder {
    agent_name: String,
    has_explicit_name: bool,
    description: Option<String>,
    skills: Vec<Skill>,
    port: u16,
    mesh_key: String,
    card: Option<AgentCard>,
    discovery_impl: Option<PeerDiscoveryImpl>,
    blocking_timeout: Duration,
}

impl AgentBuilder {
    /// Create a new builder for an agent with the given name.
    pub fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            has_explicit_name: false,
            description: None,
            skills: vec![],
            port: 0,
            mesh_key: String::new(),
            card: None,
            discovery_impl: None,
            blocking_timeout: DEFAULT_BLOCKING_TIMEOUT,
        }
    }

    /// Set the agent's description.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Set the agent's skill set.
    pub fn skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = skills;
        self
    }

    /// Set the server port (0 for OS-assigned).
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the mesh key for discovery.
    pub fn mesh_key(mut self, key: String) -> Self {
        self.mesh_key = key;
        self
    }

    /// Set the deadline for blocking `message:send` requests (spec §3.2.2).
    ///
    /// Defaults to [`DEFAULT_BLOCKING_TIMEOUT`].
    pub fn blocking_timeout(mut self, timeout: Duration) -> Self {
        self.blocking_timeout = timeout;
        self
    }

    /// Mark the agent name as explicitly provided by the user (via --name).
    /// When true, the name is used verbatim for mDNS registration. When false
    /// (the default), `-{actual_port}` is appended to prevent DNS name collisions.
    pub fn has_explicit_name(mut self, explicit: bool) -> Self {
        self.has_explicit_name = explicit;
        self
    }

    /// Use a pre-built [`AgentCard`] instead of constructing one from name,
    /// description, and skills.
    pub fn with_card(mut self, card: AgentCard) -> Self {
        self.card = Some(card);
        self
    }

    /// Inject a custom `PeerDiscoveryImpl`. Defaults to `Mdns` when not set.
    ///
    /// Use `PeerDiscoveryImpl::Noop` to run without mDNS (e.g. behind a load
    /// balancer, in CI, or in tests).
    pub fn discovery(mut self, impl_: PeerDiscoveryImpl) -> Self {
        self.discovery_impl = Some(impl_);
        self
    }

    /// Build and start the agent.
    ///
    /// Starts the HTTP server, peer discovery, and returns a fully
    /// initialised [`AgentHandle`].
    ///
    /// # Errors
    ///
    /// Returns an error tuple with the [`A2aError`] and an optional
    /// [`A2aServer`] that the caller may choose to shut down.
    pub async fn build(mut self) -> Result<AgentHandle, (A2aError, Option<Box<A2aServer>>)> {
        let mut card = match self.card {
            Some(c) => c,
            None => AgentCard {
                name: self.agent_name.clone(),
                description: self.description.clone(),
                url: "http://127.0.0.1:0".to_string(),
                provider: None,
                icon_url: None,
                documentation_url: None,
                supported_interfaces: vec![AgentInterface {
                    url: "http://127.0.0.1:0".into(),
                    protocol_version: "1.0".into(),
                    protocol_binding: crate::PROTOCOL_BINDING.into(),
                }],
                version: env!("CARGO_PKG_VERSION").to_string(),
                capabilities: AgentCapabilities::default(),
                skills: self.skills,
                security_schemes: HashMap::new(),
                extensions: vec![],
                metadata: None,
                default_input_modes: vec!["text/plain".to_string()],
                default_output_modes: vec!["text/plain".to_string()],
            },
        };

        let cache = Arc::new(PeerCache::default());

        // ── A2A completion event channel ──────────────────────────────────
        let (completion_tx, completion_rx) = mpsc::channel::<A2aCompletionEvent>(64);

        let server = match A2aServer::start_with_blocking_timeout(
            card.clone(),
            cache.clone(),
            self.port,
            self.blocking_timeout,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => return Err((e, None)),
        };
        let mut server = server;
        let task_cancel_rx = server.take_task_cancel_receiver();
        card.url = server.local_url.clone();
        if let Some(iface) = card.supported_interfaces.first_mut() {
            iface.url = server.local_url.clone();
        }

        let actual_port = server.port;

        // ── DNS-legal unique mDNS instance name ──────────────────────────────
        // When the agent name was not explicitly provided (no --name flag), append
        // the port to create a unique, DNS-legal name. This prevents mDNS from
        // renaming colliding instances to "developer (2)".
        if !self.has_explicit_name {
            card.name = format!("{}-{}", card.name, actual_port);
        }

        // ── mDNS discovery ────────────────────────────────────────────────
        let mut discovery = self
            .discovery_impl
            .take()
            .unwrap_or_else(|| PeerDiscoveryImpl::Mdns(Box::default()));
        discovery.start(&card.name, actual_port, &card, &self.mesh_key);

        // Feed peer events into the cache.
        if let Some(rx) = discovery.take_peer_rx() {
            tokio::spawn(forward_peer_events(rx, cache.clone(), card.url.clone()));
        }

        let discovery = Arc::new(Mutex::new(discovery));

        // ── Register self in peer cache ───────────────────────────────────────────
        // The agent adds itself to the peer cache so it can discover its own identity
        // via agent.list and agent.getCard tools. The port-based self-filter prevents
        // mDNS self-discovery, so we insert the entry explicitly.
        let own_peer = Peer {
            name: card.name.clone(),
            url: card.url.clone(),
            host: "127.0.0.1".to_string(),
            port: actual_port,
            card: Some(card.clone()),
            discovered_at: std::time::Instant::now(),
        };
        cache.add_or_update(own_peer);

        // ── Peer eviction ──────────────────────────────────────────────────────
        // Peers enter the cache on PeerDiscovered and leave it only on PeerLost,
        // which mDNS emits as ServiceRemoved (goodbye packet on graceful shutdown,
        // instance expiry on hard kill). No connection-failure eviction exists.
        // mdns-sd 0.20 expires an instance once its SRV record TTL lapses (SRV/A
        // records 120s, PTR/TXT records 4500s).

        let client = A2aClient::new().map_err(|e| (e, None))?;
        let card_handle = Some(server.agent_card_handle());

        // ── Periodic mDNS re-registration ──────────────────────────────────────
        let reregister_token = CancellationToken::new();
        let reregister_token_clone = reregister_token.clone();
        let discovery_clone = discovery.clone();
        let card_handle_clone = card_handle.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = reregister_token_clone.cancelled() => {
                        log::debug!("mDNS re-registration task cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        let card = match &card_handle_clone {
                            Some(handle) => handle.read().expect("card lock").clone(),
                            None => return,
                        };
                        let mut discovery = discovery_clone.lock().expect("discovery lock");
                        discovery.reregister(&card);
                    }
                }
            }
        });

        // ── Peer health sweep ──────────────────────────────────────────────────
        // mDNS silence is not proof of death: mdns-sd drops an instance from
        // every listener once its SRV TTL lapses, even while the peer's HTTP
        // server is up. Fetching the card over HTTP keeps the registry honest
        // in both directions — live peers stay, dead peers are evicted after
        // SWEEP_MAX_FAILURES consecutive failures.
        let sweep_cache = cache.clone();
        let sweep_own_url = card.url.clone();
        let sweep_token = reregister_token.clone();
        let sweep_fetcher = HttpCardFetcher::new(client.clone());
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SWEEP_INTERVAL);
            interval.tick().await;
            let mut failures: HashMap<String, u32> = HashMap::new();
            loop {
                tokio::select! {
                    _ = sweep_token.cancelled() => {
                        log::debug!("peer sweep task cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        sweep_peers(&sweep_cache, &sweep_own_url, &sweep_fetcher, &mut failures)
                            .await;
                    }
                }
            }
        });

        // ── Periodic mDNS re-query ─────────────────────────────────────────────
        // The browse retransmission delay doubles up to a 3600 s cap, so after a
        // mass eviction the next self-query can be an hour away and the cache
        // stays empty. A fresh browse re-queries the service type: every live
        // peer answers and repopulates the cache without restarting anything.
        let rediscover_token = reregister_token.clone();
        let rediscover_discovery = discovery.clone();
        let rediscover_cache = cache.clone();
        let rediscover_own_url = card.url.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(REDISCOVER_INTERVAL);
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = rediscover_token.cancelled() => {
                        log::debug!("mDNS rediscover task cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        let mut discovery = match rediscover_discovery.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        let rx = discovery.rediscover();
                        drop(discovery);
                        if let Some(rx) = rx {
                            log::debug!("mDNS rediscover started a fresh browse");
                            tokio::spawn(forward_peer_events(
                                rx,
                                rediscover_cache.clone(),
                                rediscover_own_url.clone(),
                            ));
                        }
                    }
                }
            }
        });
        Ok(AgentHandle {
            server,
            client,
            card,
            card_handle,
            cache,
            completion_tx: Some(completion_tx),
            completion_rx: Some(completion_rx),
            task_cancel_rx,
            discovery,
            mesh_key: self.mesh_key,
            reregister_token: Some(reregister_token),
        })
    }
}

// region:    --- Support

/// Interval between peer health sweeps.
pub(crate) const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// Consecutive card-fetch failures that evict a peer from the cache.
pub(crate) const SWEEP_MAX_FAILURES: u32 = 3;

/// Interval between mDNS re-queries that repopulate the peer cache.
pub(crate) const REDISCOVER_INTERVAL: Duration = Duration::from_secs(300);

/// Apply one peer event to the cache, logging the transition.
///
/// Pure cache logic — no network and no await — so the startup forwarding task
/// and every rediscover forwarding task share exactly these semantics.
pub(crate) fn apply_peer_event(cache: &PeerCache, own_url: &str, event: &PeerEvent) {
    match event {
        PeerEvent::PeerDiscovered(peer) => {
            log::info!("mesh peer discovered: {} ({})", peer.name, peer.url);
            cache.add_or_update(peer.as_ref().clone());
        }
        PeerEvent::PeerLost(name) => {
            log::warn!("mesh peer lost: {name}");
            cache.remove(name);
            let other_peers = cache
                .list()
                .iter()
                .filter(|peer| peer.url != own_url)
                .count();
            if other_peers == 0 {
                log::warn!("mesh peer cache empty — no peers besides this agent");
            }
        }
    }
}

/// Forward every event from `rx` into the cache until the channel closes.
///
/// Used for the startup browse receiver and for every receiver handed back by
/// a rediscover; the superseded task ends when its channel disconnects.
pub(crate) async fn forward_peer_events(
    mut rx: mpsc::Receiver<PeerEvent>,
    cache: Arc<PeerCache>,
    own_url: String,
) {
    while let Some(event) = rx.recv().await {
        apply_peer_event(&cache, &own_url, &event);
    }
    log::debug!(
        "peer event forwarding stopped: browse receiver closed by a rediscover or shutdown"
    );
}

/// Fetches the agent card of a peer.
///
/// Injected into [`sweep_peers`] so the sweep pass runs without HTTP in unit
/// tests; production uses [`HttpCardFetcher`].
pub(crate) trait CardFetcher {
    /// Fetch the card served under `peer_url`'s well-known path.
    fn fetch_card(
        &self,
        peer_url: &str,
    ) -> impl Future<Output = Result<AgentCard, A2aError>> + Send;
}

/// Production [`CardFetcher`] over the shared A2A HTTP client.
pub(crate) struct HttpCardFetcher {
    client: A2aClient,
}

impl HttpCardFetcher {
    pub(crate) fn new(client: A2aClient) -> Self {
        Self { client }
    }
}

impl CardFetcher for HttpCardFetcher {
    fn fetch_card(
        &self,
        peer_url: &str,
    ) -> impl Future<Output = Result<AgentCard, A2aError>> + Send {
        get_agent_card(&self.client, peer_url)
    }
}

/// Run one health-sweep pass over the cached peers.
///
/// Every peer except the agent itself gets a card fetch. A successful fetch
/// replaces the entry in place under its existing key — the mDNS instance
/// name, never the card's name, which may lack the port suffix — with a fresh
/// `discovered_at`, and clears its failure count. A peer is evicted after
/// [`SWEEP_MAX_FAILURES`] consecutive failures. `failures` is owned by the
/// sweep task so counts survive between passes.
pub(crate) async fn sweep_peers<F>(
    cache: &PeerCache,
    own_url: &str,
    fetcher: &F,
    failures: &mut HashMap<String, u32>,
) where
    F: CardFetcher,
{
    let peers: Vec<Peer> = cache
        .list()
        .into_iter()
        .filter(|peer| peer.url != own_url)
        .collect();

    if peers.is_empty() {
        return;
    }

    for peer in peers {
        let name = peer.name.clone();
        let url = peer.url.clone();

        match fetcher.fetch_card(&url).await {
            Ok(card) => {
                log::debug!("peer sweep refreshed {name} ({url})");
                failures.insert(name.clone(), 0);
                cache.add_or_update(Peer {
                    name,
                    url,
                    host: peer.host,
                    port: peer.port,
                    card: Some(card),
                    discovered_at: std::time::Instant::now(),
                });
            }
            Err(err) => {
                let consecutive = failures.entry(name.clone()).or_insert(0);
                *consecutive += 1;
                if *consecutive >= SWEEP_MAX_FAILURES {
                    failures.remove(&name);
                    cache.remove(&name);
                    log::warn!(
                        "peer sweep evicted peer {} ({}) after {} failed card fetches: {err}",
                        name,
                        url,
                        SWEEP_MAX_FAILURES
                    );
                } else {
                    log::debug!("peer sweep card fetch failed for {name} ({url}): {err}");
                }
            }
        }
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/agent/forwarding.rs"]
mod forwarding_test;

#[cfg(test)]
#[path = "../../test/agent/sweep.rs"]
mod sweep_test;

// endregion: --- Tests
