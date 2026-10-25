//! Envelope shape: version, event, timestamp, session id, payload.

use super::*;

#[test]
fn test_hooks_envelope_carries_the_v1_fields() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({"tool_calls": 3});

    // -- Exec
    let envelope = build_envelope("turn_completed", "sess-1", payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    assert_eq!(value["version"], 1, "the envelope version is 1");
    assert_eq!(value["event"], "turn_completed");
    assert_eq!(value["session_id"], "sess-1");
    assert_eq!(value["payload"]["tool_calls"], 3);
    Ok(())
}

#[test]
fn test_hooks_envelope_timestamp_is_rfc3339() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({});

    // -- Exec
    let envelope = build_envelope("session_ended", "sess-1", payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    let timestamp = value["timestamp"]
        .as_str()
        .ok_or("the envelope must carry a string timestamp")?;
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map_err(|e| format!("the timestamp must be RFC3339: {e}"))?;
    Ok(())
}

#[test]
fn test_hooks_envelope_keeps_an_empty_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({"prompt": "hi"});

    // -- Exec
    let envelope = build_envelope("turn_started", "", payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    assert_eq!(
        value["session_id"], "",
        "no active session stamps an empty id"
    );
    Ok(())
}
