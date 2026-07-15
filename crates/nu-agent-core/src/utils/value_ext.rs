use nu_protocol::Value;

/// Extract human-readable response text from a nu `Value`.
///
/// Handles:
/// - `Value::String` — returns the string directly.
/// - `Value::Record` with a `"response"` key — returns the string value.
/// - Everything else — returns the fallback `"Task completed"`.
pub fn extract_response_text_from_value(value: &Value) -> String {
    match value {
        Value::String { val, .. } => val.clone(),
        _ => value
            .as_record()
            .ok()
            .and_then(|r| r.get("response"))
            .and_then(|v| v.as_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Task completed".to_string()),
    }
}
