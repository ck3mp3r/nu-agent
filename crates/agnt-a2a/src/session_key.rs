//! Resolution of an A2A `contextId`.
//!
//! The A2A `contextId` is a client-provided opaque string. An absent id means
//! "start a fresh session" (A2A spec §3.4.1), so the server boundary mints one
//! here — the single site that owns the rule — and carries the resolved id
//! onward. The session-store key a host derives from that id is the host
//! store's vocabulary, not this crate's.

// region:    --- Functions

/// Return `context_id` unchanged, or mint a fresh UUID when it is absent, empty,
/// or whitespace only.
///
/// An absent `contextId` means "start a fresh session" (A2A spec §3.4.1). An
/// empty or whitespace-only id carries no context either, so it takes the same
/// path: carrying it would append every such send to one shared lineage. The
/// minted UUID becomes the context handle the client can resume with.
pub fn resolve_context_id(context_id: Option<String>) -> String {
    match context_id {
        Some(id) if !id.trim().is_empty() => id,
        _ => uuid::Uuid::new_v4().to_string(),
    }
}

// endregion: --- Functions
