use super::*;

// ---------------------------------------------------------------------------
// Nu preview: a nu Ask-path card carries NO producer-side content. The code
// block a gate card shows is derived by the surface from the command in the
// call's own arguments.
// ---------------------------------------------------------------------------

/// A nu Builtin call on the Ask path records a card with no producer content:
/// the command is already data in the arguments, so the drawer derives the
/// block.
#[tokio::test]
async fn interactive_resolver_nu_call_carries_no_producer_preview() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let resolver_clone = resolver.clone();
    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve(
                    "nu",
                    r#"{"command": "ls | where size > 1mb"}"#,
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
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        shows_preview,
        preview,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
    };

    // Unblock the resolver after inspecting the record.
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
    // A nu call's card exists — the drawer derives its content from the
    // command in the arguments — so the record claims a card and carries no
    // producer-side content.
    assert!(
        shows_preview,
        "nu's card exists; the drawer derives the code block from the call"
    );
    assert!(
        preview.is_none(),
        "nu carries no producer preview; the drawer derives the code block"
    );
    Ok(())
}

/// The Ask-path record must carry the exact call key
/// (`{tool_name}\n{raw_arguments}`) so the TUI can match the pending call
/// byte-for-byte. `tool` is a decorated display name and cannot be used for
/// that match.
#[tokio::test]
async fn interactive_resolver_ask_path_context_carries_exact_tool_key() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let resolver_clone = resolver.clone();
    let arguments = r#"{"command": "ls | where size > 1mb"}"#;

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        let arguments = arguments.to_string();
        async move {
            resolver
                .resolve(
                    "nu",
                    &arguments,
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
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        tool,
        call_id,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
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
    assert_eq!(
        call_id.as_str(),
        format!("nu\n{arguments}"),
        "call_id must be the exact `{{tool_name}}\\n{{raw_arguments}}` key"
    );
    assert_ne!(
        tool,
        call_id.as_str(),
        "tool is the decorated display name, not the call key"
    );
    Ok(())
}

/// Non-nu tools record no producer content on the Ask path.
#[tokio::test]
async fn interactive_resolver_other_ask_tool_has_no_pre_authorize_display() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let resolver_clone = resolver.clone();

    // -- Exec
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
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        shows_preview,
        preview,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
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
    assert!(
        !shows_preview && preview.is_none(),
        "a non-nu Ask-path record carries no producer content"
    );
    Ok(())
}

/// A nu call with a missing/non-string command still prompts normally, without
/// producer content.
#[tokio::test]
async fn interactive_resolver_nu_missing_command_prompts_without_display() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
    let resolver_clone = resolver.clone();

    // -- Exec
    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move {
            resolver
                .resolve("nu", "{}", None, bus.stream(), bus.facts(), bus.cancel())
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "Expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        preview,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
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
    assert!(
        preview.is_none(),
        "nu with a missing command must prompt without producer content"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Edit pre-authorize content: the Ask-path record must carry the diff before
// the user is prompted.
// ---------------------------------------------------------------------------

/// An edit-apply call that would create a file, on the Ask path, must record
/// the proposed diff so the user sees the change before approving or denying.
#[tokio::test]
async fn interactive_resolver_edit_apply_ask_path_carries_pre_authorize_display() -> Result<()> {
    // -- Setup & Fixtures
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("new-file.txt");
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();
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
        async move {
            resolver
                .resolve("edit", &args, None, bus.stream(), bus.facts(), bus.cancel())
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "Expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        shows_preview,
        preview,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
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
    assert!(*shows_preview, "an edit's card always has a content slot");
    let preview = preview
        .as_deref()
        .ok_or("edit apply Ask-path record must carry the proposed diff")?;
    assert!(
        preview.contains("+hello"),
        "diff must contain the created content, got: {preview:?}"
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
    let mut stream_rx = bus.stream().subscribe();
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
        async move {
            resolver
                .resolve("edit", &args, None, bus.stream(), bus.facts(), bus.cancel())
                .await
        }
    });

    let record = stream_rx
        .recv()
        .await
        .map_err(|_| "Expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        preview,
        ..
    }) = &record
    else {
        panic!("Expected a Requested permission record")
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
    let preview = preview
        .as_deref()
        .ok_or("search_replace Ask-path record must carry the proposed diff")?;
    assert!(
        preview.contains("-world") && preview.contains("+there"),
        "diff must contain the replacement, got: {preview:?}"
    );
    Ok(())
}
