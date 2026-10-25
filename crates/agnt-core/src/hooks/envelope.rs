//! The JSON envelope a hook handler receives on stdin.

use serde_json::Value;

/// The envelope schema version.
pub const ENVELOPE_VERSION: u32 = 1;

/// Build the envelope for one hook invocation.
///
/// The handler reads it with `from json` and receives a record with `version`,
/// `event`, `timestamp`, `session_id`, and `payload`.
pub fn build_envelope(event: &str, session_id: &str, payload: Value) -> String {
    serde_json::json!({
        "version": ENVELOPE_VERSION,
        "event": event,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "session_id": session_id,
        "payload": payload,
    })
    .to_string()
}
