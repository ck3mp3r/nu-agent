use std::collections::HashMap;
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
    pub async fn build(mut self) -> Result<AgentHandle, (A2aError, Option<A2aServer>)> {
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
                    protocol_binding: "HTTP+JSON".into(),
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

        let cache = Arc::new(PeerCache::new());

        // ── A2A completion event channel ──────────────────────────────────
        let (completion_tx, completion_rx) = mpsc::channel::<A2aCompletionEvent>(64);

        let server = match A2aServer::start(card.clone(), cache.clone(), self.port).await {
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
        if let Some(mut rx) = discovery.take_peer_rx() {
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
                log::warn!("peer event channel closed — agent is no longer discovering new peers");
            });
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
        // Peers are only removed on connection failure (task.send/get/cancel fails
        // with ConnectionRefused/Timeout). mDNS's 75s TTL + ServiceRemoved goodbye
        // packets handle the crash-detection path via PeerLost → cache.remove().

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
