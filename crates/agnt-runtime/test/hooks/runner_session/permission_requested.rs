//! The `permission_requested` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_requested_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequested])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.stream().publish(SessionRecord::Permission(requested()));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a permission-only hook must subscribe the stream; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "permission_requested");
    let body = payload(&calls[0])?;
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["tool"], "nu");
    assert_eq!(body["tool_key"], "nu\n{\"command\":\"ls\"}");
    assert_eq!(body["source"], "builtin");
    assert_eq!(body["mode"], "ask");
    assert_eq!(body["matched_rule_identity"], "permissions.nu");
    assert_eq!(body["scope"], "tool");
    assert_eq!(body["target_field"], "command");
    assert_eq!(body["pattern"], "ls*");
    assert_eq!(body["summary"], "run ls");
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_requested_payload_omits_pre_authorize_display() -> Result<()>
{
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequested])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.stream().publish(SessionRecord::Permission(requested()));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "the hook must have run; got: {calls:?}");
    let body = payload(&calls[0])?;
    assert!(
        body.get("pre_authorize_display").is_none(),
        "the payload must not carry the UI-only preview: {body}"
    );
    assert!(
        body.get("preview").is_none(),
        "the payload must not carry the card's content: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_requested_stamps_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequested])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
        session_id: "sess-1".to_string(),
        hydrated: false,
    });
    let _ = bus.stream().publish(SessionRecord::Permission(requested()));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the permission event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_requested_ignores_an_auto_resolved_preview() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequested])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::AutoResolved {
            rule_identity: "permissions.nu".to_string(),
            action: AutoResolution::Allow,
            reason: None,
            call_id: Some(ToolCallId::from_key("nu\n{\"command\":\"ls\"}")),
            shows_preview: true,
            preview: Some("preview".to_string()),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "an auto-approved preview must not fire permission_requested"
    );
    Ok(())
}

// region:    --- Test Support

/// A permission request with every scalar field populated and a card whose
/// content only the gate can supply.
fn requested() -> PermissionRecord {
    PermissionRecord::Requested {
        request_id: RequestId::from_minted("req-1"),
        call_id: ToolCallId::from_key("nu\n{\"command\":\"ls\"}"),
        tool: "nu".to_string(),
        source: "builtin".to_string(),
        rule_identity: "permissions.nu".to_string(),
        scope: "tool".to_string(),
        pattern: "ls*".to_string(),
        target_field: Some("command".to_string()),
        mode: Some("ask".to_string()),
        summary: "run ls".to_string(),
        shows_preview: true,
        preview: Some("preview".to_string()),
    }
}

// endregion: --- Test Support
