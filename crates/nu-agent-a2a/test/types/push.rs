use super::*;
use chrono::DateTime;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// PushNotificationConfig::created_at serialization (spec §5.6.1)
// ---------------------------------------------------------------------------

#[test]
fn push_notification_config_created_at_serializes_with_z_suffix() -> Result<()> {
    // -- Setup & Fixtures
    let config = PushNotificationConfig {
        id: "cfg-1".to_string(),
        url: "http://webhook.local/hook".to_string(),
        task_id: "task-1".to_string(),
        authentication: None,
        created_at: DateTime::from_timestamp(1_700_000_000, 0).ok_or("valid timestamp")?,
    };

    // -- Exec
    let json = serde_json::to_value(&config)?;

    // -- Check
    let ts = json["createdAt"].as_str().ok_or("createdAt string")?;
    assert!(ts.ends_with('Z'), "createdAt must end with 'Z', got: {ts}");
    assert!(
        !ts.contains('+'),
        "createdAt must not contain a timezone offset, got: {ts}"
    );
    assert_eq!(ts, "2023-11-14T22:13:20.000Z");
    Ok(())
}

#[test]
fn push_notification_config_created_at_deserializes_from_offset() -> Result<()> {
    // -- Setup & Fixtures
    let json = serde_json::json!({
        "id": "cfg-1",
        "url": "http://webhook.local/hook",
        "taskId": "task-1",
        "createdAt": "2023-11-14T22:13:20+00:00"
    });

    // -- Exec
    let config: PushNotificationConfig = serde_json::from_value(json)?;

    // -- Check
    assert_eq!(
        config.created_at,
        DateTime::from_timestamp(1_700_000_000, 0).ok_or("valid timestamp")?
    );
    Ok(())
}
