use super::*;

// ---------------------------------------------------------------------------
// InteractivePermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 4: InteractivePermissionResolver + explicit allow config → Allow, no PermissionRequested.
#[tokio::test]
async fn interactive_resolver_explicit_allow_returns_allow_no_event() {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;
    assert_eq!(decision, PermissionDecision::Allow);
    // No PermissionRequested event should have been emitted
    assert!(
        permission_rx.try_recv().is_err(),
        "Expected no PermissionRequested event for explicitly allowed tool"
    );
}

/// Test 5: InteractivePermissionResolver + explicit deny config → Deny, no PermissionRequested.
#[tokio::test]
async fn interactive_resolver_explicit_deny_returns_deny_no_event() {
    let (resolver, bus) = make_interactive(deny_all_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    // No PermissionRequested event should have been emitted
    assert!(
        permission_rx.try_recv().is_err(),
        "Expected no PermissionRequested event for explicitly denied tool"
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
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for a ToolPreview event")?
        .map_err(|_| "expected a ToolPreview event")?;
    match event {
        UiEvent::ToolPreview { tool_key, display } => {
            assert_eq!(tool_key, format!("nu\n{arguments}"));
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
    assert!(
        ui_rx.try_recv().is_err(),
        "a tool without a pre-authorize display must publish no ToolPreview"
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
    assert!(
        ui_rx.try_recv().is_err(),
        "a denied tool must publish no ToolPreview"
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
        UiEvent::PermissionRequested {
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
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);
    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    assert!(
        ui_rx.try_recv().is_err(),
        "the Ask path must not also publish a ToolPreview"
    );
    Ok(())
}

/// Test 6: InteractivePermissionResolver + ask config → PermissionRequested sent,
/// submit_decision(request_id, Allow) → resolve returns Allow.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_allow_returns_allow() -> Result<()> {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();

    // Clone the resolver so we can call submit_decision from a separate task
    let resolver_clone = resolver.clone();

    // Spawn resolve() — it will block waiting for submit_decision
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
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Allow decision
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);

    // Verify resolve() returned Allow
    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;
    assert_eq!(decision, PermissionDecision::Allow);
    Ok(())
}

/// Test 7: InteractivePermissionResolver + ask config → submit_decision(request_id, Deny) → Deny.
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
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Deny decision
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::Deny);

    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    Ok(())
}
