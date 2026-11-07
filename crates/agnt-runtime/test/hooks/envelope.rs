//! Envelope shape: version, event, timestamp, session id, agent, model, payload.

use super::*;

#[test]
fn test_hooks_envelope_carries_the_v1_fields() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({"tool_calls": 3});
    let context = HookAgentContext::default();

    // -- Exec
    let envelope = build_envelope("turn_completed", "sess-1", &context, payload);

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
    let context = HookAgentContext::default();

    // -- Exec
    let envelope = build_envelope("session_ended", "sess-1", &context, payload);

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
    let context = HookAgentContext::default();

    // -- Exec
    let envelope = build_envelope("turn_started", "", &context, payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    assert_eq!(
        value["session_id"], "",
        "no active session stamps an empty id"
    );
    Ok(())
}

#[test]
fn test_hooks_envelope_stamps_the_persona_slug_and_model() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({});
    let context = HookAgentContext::default()
        .with_persona(Some("reviewer".to_string()))
        .with_model("openai/gpt-4o".to_string());

    // -- Exec
    let envelope = build_envelope("turn_started", "sess-1", &context, payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    assert_eq!(value["agent"], "reviewer", "the slug is the agent value");
    assert_eq!(value["model"], "openai/gpt-4o");
    assert_eq!(value["version"], 1, "the version stays 1");
    Ok(())
}

#[test]
fn test_hooks_envelope_stamps_a_null_agent_without_a_persona() -> Result<()> {
    // -- Setup & Fixtures
    let payload = serde_json::json!({});
    let context = HookAgentContext::default().with_model("ollama/llama3".to_string());

    // -- Exec
    let envelope = build_envelope("turn_started", "sess-1", &context, payload);

    // -- Check
    let value: serde_json::Value = serde_json::from_str(&envelope)?;
    assert!(
        value["agent"].is_null(),
        "no persona stamps a null agent; got: {}",
        value["agent"]
    );
    assert_eq!(value["model"], "ollama/llama3");
    Ok(())
}

#[test]
fn test_hooks_envelope_reads_the_context_at_build_time() -> Result<()> {
    // -- Setup & Fixtures
    let context = HookAgentContext::default()
        .with_persona(Some("planner".to_string()))
        .with_model("openai/gpt-4o".to_string());
    let before = build_envelope("turn_started", "sess-1", &context, serde_json::json!({}));

    // -- Exec
    context.set_persona(Some("maker".to_string()));
    context.set_model("ollama/llama3".to_string());
    let after = build_envelope("turn_started", "sess-1", &context, serde_json::json!({}));

    // -- Check
    let before: serde_json::Value = serde_json::from_str(&before)?;
    let after: serde_json::Value = serde_json::from_str(&after)?;
    assert_eq!(before["agent"], "planner");
    assert_eq!(before["model"], "openai/gpt-4o");
    assert_eq!(
        after["agent"], "maker",
        "a switch reaches the next envelope"
    );
    assert_eq!(after["model"], "ollama/llama3");
    Ok(())
}
