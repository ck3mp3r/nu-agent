use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::{A2aError, AgentCard, peer::Peer};

/// Build a ServiceInfo for mDNS registration without registering it.
///
/// Extracted from [`DiscoveryService::register`] so that callers (e.g.
/// [`MdnsPeerDiscovery::rename`](crate::discovery::mdns_discovery::MdnsPeerDiscovery::rename))
/// can build a new `ServiceInfo` without going through the full register flow.
pub(crate) fn build_service_info(
    agent_name: &str,
    port: u16,
    card: &AgentCard,
    mesh_key: &str,
) -> Result<ServiceInfo, A2aError> {
    let properties = build_txt_properties(agent_name, port, card, mesh_key);

    // Convert Vec<(String, String)> to a slice of (&str, &str) refs.
    let prop_refs: Vec<(&str, &str)> = properties
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // The hostname is mandatory for the mDNS SRV record (RFC 6763)
    // but is never resolved — enable_addr_auto populates A records
    // with all host IPs (127.0.0.1 + external interfaces).
    let hostname = "a2a.local."; // static dummy, DNS-safe, no agent name dependency

    let info = ServiceInfo::new(
        "_nu-agent-a2a._tcp.local.",
        agent_name,
        hostname,
        "", // ip — empty at creation, daemon fills in via addr_auto
        port,
        // Pass the slice explicitly — `&[(K, V)]` implements
        // `IntoTxtProperties` when K, V: ToString.
        &prop_refs[..],
    )
    .map_err(|e| A2aError::Internal(format!("mdns-sd service info: {e}")))?
    .enable_addr_auto(); // daemon populates host IPs from all interfaces

    Ok(info)
}

/// Build TXT properties as key-value pairs for mDNS advertisement.
fn build_txt_properties(
    agent_name: &str,
    port: u16,
    card: &AgentCard,
    mesh_key: &str,
) -> Vec<(String, String)> {
    let mut properties = Vec::new();
    properties.push(("name".to_string(), agent_name.to_string()));
    properties.push(("version".to_string(), env!("CARGO_PKG_VERSION").to_string()));
    properties.push(("url".to_string(), format!("http://127.0.0.1:{port}")));
    properties.push(("mesh_key".to_string(), mesh_key.to_string()));

    if let Some(desc) = &card.description {
        let truncated: String = desc.chars().take(255).collect();
        properties.push(("description".to_string(), truncated));
    }
    if !card.skills.is_empty() {
        let skill_ids: String = card
            .skills
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
            .chars()
            .take(255)
            .collect();
        properties.push(("skills".to_string(), skill_ids));
    }
    properties
}

/// Extract the instance name from a full mDNS service name.
///
/// The fullname has the form `<instance>.<service_type>.<domain>`.  We split
/// at the first unescaped `.` and return the instance part.  This mirrors
/// `zeroconf`'s `ServiceDiscovery::name()` which returned just the instance.
pub(crate) fn peer_name_from_fullname(fullname: &str) -> String {
    // Simple split at first '.' — agent names rarely contain dots.  An
    // escaped dot (`\.`) in the instance portion is not handled here, but
    // mdns-sd handles it during registration.
    fullname.split('.').next().unwrap_or(fullname).to_string()
}

/// Extract the address portion of an mdns-sd [`ScopedIp`].
fn format_scoped_ip(addr: &mdns_sd::ScopedIp) -> String {
    // ScopedIp implements Display, producing either an IPv4 or IPv6 string
    // (with IPv6 scope suffix when applicable).
    addr.to_string()
}

/// Determine the mDNS instance name for an agent switch.
///
/// If the old mDNS name had a `-{port}` suffix (applied at startup when
/// `!has_explicit_name`), the new name must also carry the suffix so that
/// the mDNS service name remains unique and DNS-legal.
///
/// Returns the new mDNS instance name (without the `._nu-agent-a2a._tcp.local.`
/// suffix — that is appended by the caller).
pub fn mdns_name_for_switch(old_mdns_name: &str, new_agent_name: &str, port: u16) -> String {
    if old_mdns_name.ends_with(&format!("-{port}")) {
        format!("{new_agent_name}-{port}")
    } else {
        new_agent_name.to_string()
    }
}

// ---------------------------------------------------------------------------
// DiscoveryService — mDNS service registration
// ---------------------------------------------------------------------------

/// Registers this agent as an mDNS service on `_nu-agent-a2a._tcp.local.`.
///
/// Uses the pure-Rust [`ServiceDaemon`] from the `mdns-sd` crate — no C FFI
/// dependency (fixes SIGBUS on aarch64 Nix builds).
pub(crate) struct DiscoveryService {
    pub(crate) _daemon: Option<ServiceDaemon>,
}

impl DiscoveryService {
    /// Register a new A2A agent as an mDNS service, using a shared daemon.
    ///
    /// The daemon is shared with the [`DiscoveryBrowser`] so that both
    /// registration and browsing go through a single mDNS responder.
    pub fn register(
        daemon: ServiceDaemon,
        agent_name: &str,
        port: u16,
        card: &AgentCard,
        mesh_key: &str,
    ) -> Result<Self, A2aError> {
        let info = build_service_info(agent_name, port, card, mesh_key)?;

        daemon
            .register(info)
            .map_err(|e| A2aError::Internal(format!("mdns-sd register: {e}")))?;

        Ok(Self {
            _daemon: Some(daemon),
        })
    }
}

/// Extract the peer URL from a resolved mDNS service.
///
/// Prefers the TXT `url` property (stable across mDNS re-resolutions).
/// Falls back to constructing from the address list.
pub(crate) fn peer_url_from_service(service: &mdns_sd::ResolvedService) -> String {
    // Prefer the TXT url property — the agent publishes
    // url=http://127.0.0.1:{port} at registration time.  This is stable
    // across mDNS re-resolutions, unlike the address list which may
    // contain IPv6 link-local scoped addresses (fe80::...) that break
    // reqwest HTTP connections.
    if let Some(url) = service.get_properties().get_property_val_str("url") {
        return url.to_string();
    }

    // Fall back to constructing from address — prefer IPv4 over IPv6.
    let address = service
        .get_addresses()
        .iter()
        // Prefer non-loopback IPv4 — works for both local (server binds
        // 0.0.0.0:0) and cross-machine connections.  Loopback (127.0.0.1)
        // would fail on a remote host because it points to the wrong
        // machine's localhost.
        .find(|a| a.is_ipv4() && !a.is_loopback())
        .or_else(|| service.get_addresses().iter().find(|a| a.is_ipv4()))
        .or_else(|| service.get_addresses().iter().next())
        .map(format_scoped_ip)
        .unwrap_or_else(|| "127.0.0.1".to_string());
    format!("http://{address}:{}", service.get_port())
}

/// Convert an mdns-sd [`ResolvedService`](mdns_sd::ResolvedService) into a [`Peer`].
///
/// This extracts the fields and produces a [`Peer`] with no agent card (the
/// card is fetched separately in the browse loop).
pub(crate) fn peer_from_service(service: &mdns_sd::ResolvedService) -> Peer {
    let name = peer_name_from_fullname(service.get_fullname());
    let url = peer_url_from_service(service);

    Peer {
        name,
        url,
        host: service.get_hostname().to_string(),
        port: service.get_port(),
        card: None,
        discovered_at: std::time::Instant::now(),
    }
}
