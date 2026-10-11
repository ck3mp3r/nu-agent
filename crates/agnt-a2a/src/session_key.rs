//! Session-key derivation for A2A `contextId`-based routing.
//!
//! The A2A `contextId` is a client-provided opaque string. The internal
//! session-store key is derived from it as `{prefix}-{context_id}`, where
//! `prefix` is the path-hash prefix that scopes sessions to a working
//! directory. The key format IS the mapping — no registry is needed.

// region:    --- Functions

/// Derive the internal session-store key for an A2A `contextId`.
///
/// `prefix` is the path-hash prefix from `dir_prefix(cwd)`. The returned key
/// is `{prefix}-{context_id}`.
///
/// # Arguments
/// * `prefix` - Path-hash prefix scoping sessions to a working directory
/// * `context_id` - Client-provided A2A context identifier
///
pub fn derive_session_key(prefix: &str, context_id: &str) -> String {
    format!("{prefix}-{context_id}")
}

/// Return `context_id` unchanged, or mint a fresh UUID when it is absent.
///
/// An absent `contextId` means "start a fresh session" (A2A spec §3.4.1). The
/// minted UUID becomes the context handle the client can resume with.
pub fn resolve_context_id(context_id: Option<String>) -> String {
    context_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

// endregion: --- Functions
