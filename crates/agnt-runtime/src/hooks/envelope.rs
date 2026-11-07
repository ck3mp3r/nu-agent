//! The JSON envelope a hook handler receives on stdin.

use serde_json::Value;

use super::context::HookAgentContext;

/// The envelope schema version.
pub const ENVELOPE_VERSION: u32 = 1;

/// Build the envelope for one hook invocation.
///
/// The handler reads it with `from json` and receives a record with `version`,
/// `event`, `timestamp`, `session_id`, `agent`, `model`, and `payload`.
///
/// `agent` is the persona slug, or `null` when no persona is loaded. `model` is
/// the active model identity (`provider/model`). Both are read from `context`
/// at build time, so a persona or model switch reaches the next envelope.
pub fn build_envelope(
    event: &str,
    session_id: &str,
    context: &HookAgentContext,
    payload: Value,
) -> String {
    serde_json::json!({
        "version": ENVELOPE_VERSION,
        "event": event,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "session_id": session_id,
        "agent": context.persona(),
        "model": context.model(),
        "payload": payload,
    })
    .to_string()
}
