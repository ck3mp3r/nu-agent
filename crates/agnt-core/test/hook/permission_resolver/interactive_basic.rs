use super::*;

// ---------------------------------------------------------------------------
// InteractivePermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 4: InteractivePermissionResolver + explicit allow config → Allow, no prompt.
#[tokio::test]
async fn interactive_resolver_explicit_allow_returns_allow_no_prompt() {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut permission_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionRequested { .. })),
        "an explicitly allowed tool must not prompt: {events:?}"
    );
}

/// Test 5: InteractivePermissionResolver + explicit deny config → Deny, no prompt.
#[tokio::test]
async fn interactive_resolver_explicit_deny_returns_deny_no_prompt() {
    let (resolver, bus) = make_interactive(deny_all_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let events = drain(&mut permission_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionRequested { .. })),
        "an explicitly denied tool must not prompt: {events:?}"
    );
}

/// Test 5b: auto-approve of a previewable tool publishes `ToolPreview` and
/// records the call key in `previewed`.
#[tokio::test]
async fn interactive_auto_approve_publishes_tool_preview() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(nu_allowed_config());
    let mut ui_rx = bus.ui_event().subscribe();
    let arguments = r#"{"command":"ls"}"#;

    // -- Exec
    let decision = resolver.resolve("nu", arguments, None, &bus).await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    // Bounded: the bus keeps its sender alive, so a missing event would
    // otherwise block this test forever instead of failing.
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the auto-resolved event")?
        .map_err(|_| "expected an auto-resolved event")?;
    assert!(
        matches!(first, SystemEvent::PermissionAutoResolved { .. }),
        "the auto-resolved event precedes the preview: {first:?}"
    );
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for a ToolPreview event")?
        .map_err(|_| "expected a ToolPreview event")?;
    match event {
        SystemEvent::PreviewRequested { call, display } => {
            assert_eq!(call.id.as_str(), format!("nu\n{arguments}"));
            assert_eq!(display.title, None);
            assert_eq!(display.sections.len(), 1);
            assert_eq!(display.sections[0].content, "ls");
        }
        other => panic!("expected ToolPreview, got {other:?}"),
    }
    assert!(
        resolver.take_previewed("nu", arguments),
        "the auto-approve path must record the call key in previewed"
    );
    assert!(
        !resolver.take_previewed("nu", arguments),
        "take_previewed must consume the key"
    );
    Ok(())
}

/// Test 5c: auto-approve of a non-previewable tool publishes no `ToolPreview`.
#[tokio::test]
async fn interactive_auto_approve_non_previewable_tool_publishes_no_tool_preview() {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut ui_rx = bus.ui_event().subscribe();
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PreviewRequested { .. })),
        "a tool without a pre-authorize display must publish no ToolPreview: {events:?}"
    );
}

/// Test 5d: a denied tool publishes no `ToolPreview`.
#[tokio::test]
async fn interactive_deny_publishes_no_tool_preview() {
    let (resolver, bus) = make_interactive(deny_all_toml_config());
    let mut ui_rx = bus.ui_event().subscribe();
    let decision = resolver
        .resolve("nu", r#"{"command":"ls"}"#, None, &bus)
        .await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PreviewRequested { .. })),
        "a denied tool must publish no ToolPreview: {events:?}"
    );
}

/// Test 5e: the Ask path publishes `PermissionRequested` and never a
/// `ToolPreview` for the same call.
#[tokio::test]
async fn interactive_ask_path_publishes_permission_requested_not_tool_preview() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut ui_rx = bus.ui_event().subscribe();
    let arguments = r#"{"command":"ls"}"#;
    let resolver_clone = resolver.clone();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve("nu", arguments, None, &bus).await }
    });

    let event = ui_rx
        .recv()
        .await
        .map_err(|_| "expected PermissionRequested event")?;
    let request_id = match event {
        SystemEvent::PermissionRequested {
            request_id,
            context,
        } => {
            assert!(
                context.pre_authorize_display.is_some(),
                "the Ask path must carry the pre-authorize display"
            );
            request_id
        }
        other => panic!("expected PermissionRequested, got {other:?}"),
    };
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::AllowOnce,
    );
    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let mut events = Vec::new();
    while let Ok(event) = ui_rx.try_recv() {
        events.push(event);
    }
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PreviewRequested { .. })),
        "the Ask path must not also publish a ToolPreview: {events:?}"
    );
    Ok(())
}

/// Test 6: InteractivePermissionResolver + ask config → PermissionRequested sent,
/// the test-support `submit_decision` helper (request_id, Allow) → resolve returns Allow.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_allow_returns_allow() -> Result<()> {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();

    // Clone the resolver so the test-support helper can submit from a separate task
    let resolver_clone = resolver.clone();

    // Spawn resolve() — it will block until the decision is submitted
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve(ASK_TOOL, "{}", None, &bus).await }
    });

    // Wait for the PermissionRequested event
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Allow decision
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::AllowOnce,
    );

    // Verify resolve() returned Allow
    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;
    assert_eq!(decision, PermissionDecision::Allow);
    Ok(())
}

/// Test 7: InteractivePermissionResolver + ask config → test-support submit_decision(request_id, Deny) → Deny.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_deny_returns_deny() -> Result<()> {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();

    let resolver_clone = resolver.clone();

    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve(ASK_TOOL, "{}", None, &bus).await }
    });

    // Wait for the PermissionRequested event
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Deny decision
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::Deny,
    );

    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    Ok(())
}
