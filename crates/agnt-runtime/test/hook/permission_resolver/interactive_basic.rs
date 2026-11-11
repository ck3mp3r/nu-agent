use super::*;

// ---------------------------------------------------------------------------
// InteractivePermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 4: InteractivePermissionResolver + explicit allow config → Allow, no prompt.
#[tokio::test]
async fn interactive_resolver_explicit_allow_returns_allow_no_prompt() {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let decision = resolver
        .resolve(
            ALLOW_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "an explicitly allowed tool must not prompt: {records:?}"
    );
}

/// Test 5: InteractivePermissionResolver + explicit deny config → Deny, no prompt.
#[tokio::test]
async fn interactive_resolver_explicit_deny_returns_deny_no_prompt() {
    let (resolver, bus) = make_interactive(deny_all_config());
    let mut stream_rx = bus.stream().subscribe();
    let decision = resolver
        .resolve(
            ASK_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "an explicitly denied tool must not prompt: {records:?}"
    );
}

/// Test 5c: an auto-approved call records its resolution and opens no card.
#[tokio::test]
async fn interactive_auto_approve_records_resolution_without_a_card() {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let decision = resolver
        .resolve(
            ALLOW_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    assert!(
        records
            .iter()
            .any(|record| matches!(record, PermissionRecord::AutoResolved { .. })),
        "the auto-approval records its own resolution: {records:?}"
    );
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "an auto-approved call opens no card: {records:?}"
    );
}

/// Test 5d: a denied tool opens no card.
#[tokio::test]
async fn interactive_deny_opens_no_card() {
    let (resolver, bus) = make_interactive(deny_all_toml_config());
    let mut stream_rx = bus.stream().subscribe();
    let decision = resolver
        .resolve(
            "nu",
            r#"{"command":"ls"}"#,
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "a denied tool opens no card: {records:?}"
    );
}

/// Test 5e: the Ask path records the card and never an auto-resolution for the
/// same call.
#[tokio::test]
async fn interactive_ask_path_records_a_card_not_an_auto_resolution() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let arguments = r#"{"command":"ls"}"#;
    let resolver_clone = resolver.clone();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve(
                    "nu",
                    arguments,
                    None,
                    bus.stream(),
                    bus.facts(),
                    bus.cancel(),
                )
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        preview,
        ..
    }) = &record
    else {
        panic!("expected a Requested permission record")
    };
    assert!(
        preview.is_none(),
        "the card is the surface's derivation; the Ask path adds no content it cannot compute"
    );
    let request_id = request_id.clone();
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
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::AutoResolved { .. })),
        "the Ask path must not also record an auto-resolution: {records:?}"
    );
    Ok(())
}

/// Test 6: InteractivePermissionResolver + ask config → the gate records the
/// request, `submit_decision` answers it → resolve returns Allow.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_allow_returns_allow() -> Result<()> {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();

    let resolver_clone = resolver.clone();

    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve(
                    ASK_TOOL,
                    "{}",
                    None,
                    bus.stream(),
                    bus.facts(),
                    bus.cancel(),
                )
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "Expected a gate record")?;
    let request_id = request_id_of(&permissions(vec![record]))?;

    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::AllowOnce,
    );

    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;
    assert_eq!(decision, PermissionDecision::Allow);
    Ok(())
}

/// Test 7: InteractivePermissionResolver + ask config → `submit_decision(Deny)` → Deny.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_deny_returns_deny() -> Result<()> {
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();

    let resolver_clone = resolver.clone();

    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve(
                    ASK_TOOL,
                    "{}",
                    None,
                    bus.stream(),
                    bus.facts(),
                    bus.cancel(),
                )
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "Expected a gate record")?;
    let request_id = request_id_of(&permissions(vec![record]))?;

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
