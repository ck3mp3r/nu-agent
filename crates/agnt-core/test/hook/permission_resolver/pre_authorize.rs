use super::*;

// ---------------------------------------------------------------------------
// Nu preview (task 69af3012): nu Ask-path requests carry a pre_authorize_display
// code block built from the tool-call arguments alone.
// ---------------------------------------------------------------------------

/// A nu Builtin call on the Ask path publishes a context whose
/// pre_authorize_display is a single `ContentKind::Code { language: "nu" }`
/// section carrying the command.
#[tokio::test]
async fn interactive_resolver_nu_call_carries_pre_authorize_display() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let command = "ls | where size > 1mb";

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve("nu", r#"{"command": "ls | where size > 1mb"}"#, None, &bus)
                .await
        }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };

    // Unblock the resolver after inspecting the event.
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
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    let display = context
        .pre_authorize_display
        .as_ref()
        .ok_or("nu Ask-path context must carry a pre-authorize display")?;
    assert_eq!(display.title, None);
    let section = display
        .sections
        .first()
        .ok_or("pre-authorize display should have one section")?;
    assert_eq!(
        section.kind,
        crate::transcript::ir::ContentKind::Code {
            language: "nu".to_string()
        }
    );
    assert_eq!(section.content, command);
    Ok(())
}

/// The Ask-path context must carry the exact call key
/// (`{tool_name}\n{raw_arguments}`) so the TUI can match the pending call
/// byte-for-byte. `tool` is a decorated display name and cannot be used for
/// that match.
#[tokio::test]
async fn interactive_resolver_ask_path_context_carries_exact_tool_key() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let arguments = r#"{"command": "ls | where size > 1mb"}"#;

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        let arguments = arguments.to_string();
        async move { resolver.resolve("nu", &arguments, None, &bus).await }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::Deny,
    );
    let _decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    assert_eq!(
        context.tool_key,
        format!("nu\n{arguments}"),
        "tool_key must be the exact `{{tool_name}}\\n{{raw_arguments}}` key"
    );
    assert_ne!(
        context.tool, context.tool_key,
        "tool is the decorated display name, not the call key"
    );
    Ok(())
}

/// Non-nu tools keep `pre_authorize_display: None` on the Ask path.
#[tokio::test]
async fn interactive_resolver_other_ask_tool_has_no_pre_authorize_display() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve(ASK_TOOL, "{}", None, &bus).await }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
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
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    assert!(
        context.pre_authorize_display.is_none(),
        "non-nu Ask-path context must not carry a pre-authorize display"
    );
    Ok(())
}

/// A nu call with a missing/non-string command still prompts normally, without a display.
#[tokio::test]
async fn interactive_resolver_nu_missing_command_prompts_without_display() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve("nu", "{}", None, &bus).await }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
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
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    assert!(
        context.pre_authorize_display.is_none(),
        "nu with missing command must prompt without a preview"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Edit pre-authorize preview: the Ask path must carry a diff before the
// user is prompted (regression: pre_authorize was never wired into the
// interactive resolver, so edit prompts had no preview at all).
// ---------------------------------------------------------------------------

/// An edit-apply call that would create a file, on the Ask path, must carry
/// a `pre_authorize_display` diff so the user sees the change before
/// approving/denying — mirroring the coverage already present for the
/// no-preview (nu) case above.
#[tokio::test]
async fn interactive_resolver_edit_apply_ask_path_carries_pre_authorize_display() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("new-file.txt");
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let args = serde_json::json!({
        "path": target.to_string_lossy(),
        "mode": "apply",
        "operation": {"type": "create", "content": "hello\n"}
    })
    .to_string();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve("edit", &args, None, &bus).await }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::Deny,
    );
    let _decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    let display = context
        .pre_authorize_display
        .as_ref()
        .ok_or("edit apply Ask-path context must carry a pre-authorize display")?;
    let section = display
        .sections
        .first()
        .ok_or("pre-authorize display should have one section")?;
    assert!(
        matches!(
            section.kind,
            crate::transcript::ir::ContentKind::Diff { .. }
        ),
        "section kind must be Diff, got {:?}",
        section.kind
    );
    assert!(
        section.content.contains("+hello"),
        "diff must contain the created content, got: {:?}",
        section.content
    );
    Ok(())
}

/// A search_replace edit-apply call that omits `expected_version` (the edit
/// tool schema does not require it) must still carry a diff on the Ask path.
/// Pre-authorize resolves the preview version from the file on disk.
#[tokio::test]
async fn interactive_resolver_edit_apply_search_replace_without_expected_version_carries_pre_authorize_display()
-> Result<()> {
    // -- Setup & Fixtures
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("existing.txt");
    std::fs::write(&target, "hello\nworld\n")?;
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let args = serde_json::json!({
        "path": target.to_string_lossy(),
        "mode": "apply",
        "operation": {
            "type": "search_replace",
            "search": "world",
            "replacement": "there"
        }
    })
    .to_string();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve("edit", &args, None, &bus).await }
    });

    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        SystemEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    submit_decision(
        &resolver_clone,
        request_id.as_str(),
        ProtocolPermissionDecision::Deny,
    );
    let _decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    let SystemEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    let display = context
        .pre_authorize_display
        .as_ref()
        .ok_or("search_replace Ask-path context must carry a pre-authorize display")?;
    let section = display
        .sections
        .first()
        .ok_or("pre-authorize display should have one section")?;
    assert!(
        matches!(
            section.kind,
            crate::transcript::ir::ContentKind::Diff { .. }
        ),
        "section kind must be Diff, got {:?}",
        section.kind
    );
    assert!(
        section.content.contains("-world") && section.content.contains("+there"),
        "diff must contain the replacement, got: {:?}",
        section.content
    );
    Ok(())
}
