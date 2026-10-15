use super::*;

/// An explicitly allowed tool (read) still returns Allow without emitting any
/// PermissionRequested event — no preview path involvement.
#[tokio::test]
async fn interactive_resolver_allowed_tool_emits_no_permission_event_with_display() {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();

    // -- Exec
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    assert!(
        permission_rx.try_recv().is_err(),
        "allowed tool must not emit a PermissionRequested event"
    );
}

// ---------------------------------------------------------------------------
// take_previewed: the single, race-free source of truth `HookChain` consults
// after the tool finishes, to decide whether the completion event should
// still carry its own copy of an already-shown preview.
// ---------------------------------------------------------------------------

/// After an edit-apply Ask-path call that carried a preview, `take_previewed`
/// reports `true` exactly once for that `(tool_name, arguments)` pair, then
/// `false` on subsequent calls (single-use, matching a single tool
/// invocation).
#[tokio::test]
async fn interactive_resolver_take_previewed_is_true_once_after_edit_preview() -> Result<()> {
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
        let args = args.clone();
        async move { resolver.resolve("edit", &args, None, &bus).await }
    });
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        UiEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);
    resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        resolver_clone.take_previewed("edit", &args),
        "a preview was shown for this exact call; take_previewed must report true"
    );
    assert!(
        !resolver_clone.take_previewed("edit", &args),
        "take_previewed must be single-use: false on the second call"
    );
    Ok(())
}

/// A nu Ask-path call that carried a preview reports `take_previewed` true
/// exactly once for that `(tool_name, arguments)` pair, then false.
#[tokio::test]
async fn interactive_resolver_take_previewed_is_true_once_after_nu_preview() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let args = r#"{"command": "ls"}"#;

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve("nu", r#"{"command": "ls"}"#, None, &bus)
                .await
        }
    });
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        UiEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);
    resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        resolver_clone.take_previewed("nu", args),
        "a preview was shown for this exact call; take_previewed must report true"
    );
    assert!(
        !resolver_clone.take_previewed("nu", args),
        "take_previewed must be single-use: false on the second call"
    );
    Ok(())
}

/// A nu call whose arguments carry no command produces no preview, so
/// `take_previewed` stays false — nothing to consume.
#[tokio::test]
async fn interactive_resolver_take_previewed_is_false_without_a_nu_preview() -> Result<()> {
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
        UiEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);
    resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        !resolver_clone.take_previewed("nu", "{}"),
        "a nu call without a command shows no preview; take_previewed must report false"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// A UI that does not render previews (ACP): the resolver must record nothing,
// so `take_previewed` stays false and `HookChain::on_outcome` keeps the
// completion display — the ACP client receives the diff.
// ---------------------------------------------------------------------------

/// Auto-approve of a previewable tool for a UI that does not render previews
/// (ACP) publishes no `ToolPreview` and records no key, so the completion
/// display is preserved.
#[tokio::test]
async fn interactive_resolver_without_preview_rendering_keeps_completion_display_after_auto_approve()
-> Result<()> {
    // -- Setup & Fixtures
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (resolver, bus) = make_interactive_with_cwd_and_previews(nu_allowed_config(), cwd, false);
    let mut ui_rx = bus.ui_event().subscribe();
    let arguments = r#"{"command":"ls"}"#;

    // -- Exec
    let decision = resolver.resolve("nu", arguments, None, &bus).await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    assert!(
        ui_rx.try_recv().is_err(),
        "a UI that does not render previews must publish no ToolPreview"
    );
    assert!(
        !resolver.take_previewed("nu", arguments),
        "no preview was rendered, so the completion display must be preserved"
    );
    Ok(())
}

/// The Ask path for a UI that does not render previews (ACP) records no key,
/// so `take_previewed` stays false and the completion display is preserved.
#[tokio::test]
async fn interactive_resolver_without_preview_rendering_keeps_completion_display_after_ask()
-> Result<()> {
    // -- Setup & Fixtures
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (resolver, bus) =
        make_interactive_with_cwd_and_previews(ask_global_with_read_allowed_config(), cwd, false);
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let arguments = r#"{"command":"ls"}"#;

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve("nu", arguments, None, &bus).await }
    });
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match &event {
        UiEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);
    resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        !resolver_clone.take_previewed("nu", arguments),
        "no preview was rendered, so the completion display must be preserved"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// cwd source: the resolver must use the cwd passed to `new()`, not the plugin
// process cwd (`std::env::current_dir()`), which is frozen at plugin spawn.
// ---------------------------------------------------------------------------

/// Regression test: a relative-path edit-apply call with no `expected_version`
/// must produce a preview when the file exists under the resolver's cwd — even
/// though that cwd differs from `std::env::current_dir()`.
///
/// Before the fix, `resolve()` read the file relative to the plugin process
/// cwd, the read failed, `preview_version` was None, and
/// `plan_search_replace_edit` returned `Err(MissingExpectedVersion)` — so the
/// `PermissionRequested` event carried no preview.
#[tokio::test]
async fn interactive_resolver_uses_passed_cwd_not_process_cwd() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("existing.txt");
    std::fs::write(&target, "hello\nworld\n")?;
    let process_cwd = std::env::current_dir()?;
    assert_ne!(
        process_cwd,
        tmp.path(),
        "test is vacuous if the process cwd equals the tempdir"
    );
    let (resolver, bus) = make_interactive_with_cwd(
        ask_global_with_read_allowed_config(),
        tmp.path().to_path_buf(),
    );
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();
    let args = serde_json::json!({
        "path": "existing.txt",
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
        UiEvent::PermissionRequested { request_id, .. } => request_id.clone(),
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::Deny);
    let _decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    let UiEvent::PermissionRequested { context, .. } = &event else {
        panic!("Expected PermissionRequested event")
    };
    let display = context
        .pre_authorize_display
        .as_ref()
        .ok_or("relative-path edit must resolve against the resolver's cwd")?;
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
