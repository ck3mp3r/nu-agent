use mdns_sd::TxtProperties;

/// Reason a peer was rejected by a filter.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterRejection {
    /// The resolved service has 127.0.0.1 in its addresses and its port
    /// matches ours — it's our own mDNS registration.
    SelfPortMatch { port: u16 },
    /// The peer advertises a different mesh key.
    MeshKeyMismatch { peer_key: String, our_key: String },
    /// The peer has no mesh_key in its TXT records.
    MissingMeshKey { fullname: String },
}

impl std::fmt::Display for FilterRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterRejection::SelfPortMatch { port } => {
                write!(f, "self-match (port {port} with localhost)")
            }
            FilterRejection::MeshKeyMismatch { peer_key, our_key } => {
                write!(
                    f,
                    "mesh key mismatch: peer has {peer_key}, we have {our_key}"
                )
            }
            FilterRejection::MissingMeshKey { fullname } => {
                write!(f, "peer {fullname} missing mesh_key in TXT records")
            }
        }
    }
}

/// Concrete filter types available in the filter chain.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Reject peers whose port matches our own AND have 127.0.0.1 in their
    /// resolved addresses. This identifies the agent's own mDNS registration
    /// regardless of what mDNS renames the instance name to.
    SelfPort(u16),
    /// Reject peers whose mesh_key differs from ours.
    MeshKey(String),
}

/// Result of a single filter check.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterDecision {
    Accept,
    Reject(FilterRejection),
}

/// Reject if the resolved service has 127.0.0.1 in its addresses AND its
/// port matches our own port.
///
/// `peer_has_localhost` is true when `resolved.get_addresses()` contains
/// `127.0.0.1`. This is only true for services on the same machine —
/// remote machines never include 127.0.0.1 in their mDNS responses.
pub(crate) fn check_self_port_filter(
    own_port: u16,
    peer_port: u16,
    peer_has_localhost: bool,
) -> FilterDecision {
    if peer_has_localhost && peer_port == own_port {
        FilterDecision::Reject(FilterRejection::SelfPortMatch { port: own_port })
    } else {
        FilterDecision::Accept
    }
}

/// Check whether a resolved peer's mesh_key matches our own.
///
/// Returns `Accept` if the peer's mesh_key matches ours.
/// Returns `MeshKeyMismatch` if the peer has a different key.
/// Returns `MissingMeshKey` if the peer has no mesh_key property.
pub(crate) fn check_mesh_key_filter(mesh_key: &str, props: &TxtProperties) -> FilterDecision {
    let peer_key = props.get_property_val_str("mesh_key");
    match peer_key {
        Some(k) if k != mesh_key => FilterDecision::Reject(FilterRejection::MeshKeyMismatch {
            peer_key: k.to_string(),
            our_key: mesh_key.to_string(),
        }),
        Some(_) => FilterDecision::Accept,
        None => FilterDecision::Reject(FilterRejection::MissingMeshKey {
            fullname: "unknown".to_string(),
        }),
    }
}

/// Run all filters in order.  Returns `Ok(())` if all filters accept, or the
/// first [`FilterRejection`] reason.
pub fn check_all(
    filters: &[Filter],
    _fullname: &str,
    props: &TxtProperties,
    peer_port: u16,
    peer_has_localhost: bool,
) -> Result<(), FilterRejection> {
    for filter in filters {
        let decision = match filter {
            Filter::SelfPort(own_port) => {
                check_self_port_filter(*own_port, peer_port, peer_has_localhost)
            }
            Filter::MeshKey(key) => check_mesh_key_filter(key, props),
        };
        match decision {
            FilterDecision::Accept => continue,
            FilterDecision::Reject(reason) => return Err(reason),
        }
    }
    Ok(())
}

/// Build the default filter chain for an A2A agent.
///
/// Filters are evaluated in order:
/// 1. Self-exclusion — skip our own mDNS announcements.
/// 2. Mesh-key scoping — only accept peers in the same mesh.
pub fn build_filters(own_port: u16, mesh_key: &str) -> Vec<Filter> {
    vec![
        Filter::SelfPort(own_port),
        Filter::MeshKey(mesh_key.to_string()),
    ]
}
