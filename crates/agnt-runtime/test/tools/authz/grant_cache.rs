use super::*;

// ── SessionGrantCache::clear tests ──────────────────────────────────────

#[test]
fn clear_empties_all_grants() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();

    // Insert grants for multiple tools
    let shell_args = args_map!({"command": "echo one"});
    let shell_decision = parsed.evaluate("shell", &shell_args);
    let _ = apply_ask_choice(
        shell_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &shell_args,
    );

    let read_args = args_map!({"filePath": "README.md"});
    let read_decision = parsed.evaluate("read", &read_args);
    let _ = apply_ask_choice(
        read_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "read",
        "closure",
        &read_args,
    );

    let glob_args = args_map!({"pattern": "**/*.rs"});
    let glob_decision = parsed.evaluate("glob", &glob_args);
    let _ = apply_ask_choice(
        glob_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "glob",
        "closure",
        &glob_args,
    );

    // Verify grants exist before clear
    let shell_check = parsed.evaluate("shell", &shell_args);
    assert_eq!(
        apply_session_grant_override(shell_check, &cache, "shell", "closure", &shell_args).action,
        PermissionAction::Allow
    );

    let glob_check = parsed.evaluate("glob", &glob_args);
    assert_eq!(
        apply_session_grant_override(glob_check, &cache, "glob", "closure", &glob_args).action,
        PermissionAction::Allow
    );

    // Clear all grants
    cache.clear();

    // Verify all grants are gone — only check tools whose config action is Ask
    // (read is configured as Allow in the fixture, so it would still return Allow
    // after cache clear — that's correct behavior, not a cache leak)
    let shell_after = parsed.evaluate("shell", &shell_args);
    assert_eq!(
        apply_session_grant_override(shell_after, &cache, "shell", "closure", &shell_args).action,
        PermissionAction::Ask
    );

    let glob_after = parsed.evaluate("glob", &glob_args);
    assert_eq!(
        apply_session_grant_override(glob_after, &cache, "glob", "closure", &glob_args).action,
        PermissionAction::Ask
    );
}

#[test]
fn clear_on_empty_cache() {
    let mut cache = SessionGrantCache::default();

    // Should not panic
    cache.clear();

    // Verify still empty
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let args = args_map!({"command": "echo test"});
    let decision = parsed.evaluate("shell", &args);
    assert_eq!(
        apply_session_grant_override(decision, &cache, "shell", "closure", &args).action,
        PermissionAction::Ask
    );
}

// ── SessionGrantCache::clear_for_server tests ─────────────────────────────

/// `clear_for_server` removes exactly the grants whose exposed tool name
/// carries the `<server>__` prefix, and leaves every other grant in place.
#[test]
fn clear_for_server_table() {
    // -- Setup & Fixtures
    struct Grant {
        tool: &'static str,
        source: &'static str,
        args: serde_json::Map<String, serde_json::Value>,
    }

    struct Case {
        name: &'static str,
        grants: Vec<Grant>,
        cleared_server: &'static str,
        expected: Vec<(&'static str, &'static str, PermissionAction)>,
    }

    let cases = vec![
        Case {
            name: "removes only the matching prefix",
            grants: vec![
                Grant {
                    tool: "context7__search",
                    source: "mcp",
                    args: args_map!({"query": "test"}),
                },
                Grant {
                    tool: "context7__fetch",
                    source: "mcp",
                    args: args_map!({"url": "https://example.com"}),
                },
                Grant {
                    tool: "gh__list_prs",
                    source: "mcp",
                    args: args_map!({}),
                },
                Grant {
                    tool: "shell",
                    source: "closure",
                    args: args_map!({"command": "echo test"}),
                },
            ],
            cleared_server: "context7",
            expected: vec![
                ("context7__search", "mcp", PermissionAction::Ask),
                ("context7__fetch", "mcp", PermissionAction::Ask),
                ("gh__list_prs", "mcp", PermissionAction::Allow),
                ("shell", "closure", PermissionAction::Allow),
            ],
        },
        Case {
            name: "no matching grants leaves the others intact",
            grants: vec![
                Grant {
                    tool: "gh__list_prs",
                    source: "mcp",
                    args: args_map!({}),
                },
                Grant {
                    tool: "gh__get_pr",
                    source: "mcp",
                    args: args_map!({"number": 1}),
                },
            ],
            cleared_server: "context7",
            expected: vec![
                ("gh__list_prs", "mcp", PermissionAction::Allow),
                ("gh__get_pr", "mcp", PermissionAction::Allow),
            ],
        },
        Case {
            name: "empty cache is a no-op",
            grants: vec![],
            cleared_server: "context7",
            expected: vec![("shell", "closure", PermissionAction::Ask)],
        },
        Case {
            name: "exact prefix match only",
            grants: vec![
                Grant {
                    tool: "context7__search",
                    source: "mcp",
                    args: args_map!({"query": "test"}),
                },
                Grant {
                    tool: "context7x__search",
                    source: "mcp",
                    args: args_map!({"query": "other"}),
                },
            ],
            cleared_server: "context7",
            expected: vec![
                ("context7__search", "mcp", PermissionAction::Ask),
                ("context7x__search", "mcp", PermissionAction::Allow),
            ],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
        let mut cache = SessionGrantCache::default();
        for grant in &case.grants {
            let decision = parsed.evaluate(grant.tool, &grant.args);
            let _ = apply_ask_choice(
                decision,
                AskChoice::AllowAlways,
                &mut cache,
                grant.tool,
                grant.source,
                &grant.args,
            );
        }

        // Grants are live before the clear.
        for grant in &case.grants {
            let decision = parsed.evaluate(grant.tool, &grant.args);
            assert_eq!(
                apply_session_grant_override(
                    decision,
                    &cache,
                    grant.tool,
                    grant.source,
                    &grant.args
                )
                .action,
                PermissionAction::Allow,
                "case '{}': {} must hold a grant before clear_for_server",
                case.name,
                grant.tool
            );
        }

        cache.clear_for_server(case.cleared_server);

        for (tool, source, expected) in &case.expected {
            let args = case
                .grants
                .iter()
                .find(|grant| grant.tool == *tool)
                .map(|grant| grant.args.clone())
                .unwrap_or_else(|| args_map!({"command": "echo test"}));
            let decision = parsed.evaluate(tool, &args);
            assert_eq!(
                apply_session_grant_override(decision, &cache, tool, source, &args).action,
                *expected,
                "case '{}': {tool} must be {expected:?} after clear_for_server",
                case.name
            );
        }
    }
}

// ── SessionGrantCache mode-agnostic grant tests ───────────────────────────

/// The `mode` tool argument is not part of the grant scope. A grant made with
/// one mode value covers later calls with any other mode value, and a grant
/// made without a mode covers later calls that carry one.
#[test]
fn grant_covers_call_with_different_mode() {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        tool: &'static str,
        granted_args: serde_json::Map<String, serde_json::Value>,
        checked_args: serde_json::Map<String, serde_json::Value>,
    }

    let cases = vec![
        Case {
            name: "granted preview covers apply",
            tool: "edit",
            granted_args: args_map!({"path": "file.txt", "mode": "preview"}),
            checked_args: args_map!({"path": "file.txt", "mode": "apply"}),
        },
        Case {
            name: "granted apply covers preview",
            tool: "edit",
            granted_args: args_map!({"path": "file.txt", "mode": "apply"}),
            checked_args: args_map!({"path": "file.txt", "mode": "preview"}),
        },
        Case {
            name: "granted without mode covers markdown",
            tool: "http",
            granted_args: args_map!({"url": "https://example.com"}),
            checked_args: args_map!({"url": "https://example.com", "mode": "markdown"}),
        },
        Case {
            name: "granted markdown covers raw",
            tool: "http",
            granted_args: args_map!({"url": "https://example.com", "mode": "markdown"}),
            checked_args: args_map!({"url": "https://example.com", "mode": "raw"}),
        },
    ];

    // -- Exec & Check
    for case in cases {
        let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
        let mut cache = SessionGrantCache::default();

        let granted = parsed.evaluate(case.tool, &case.granted_args);
        assert_eq!(
            granted.action,
            PermissionAction::Ask,
            "case '{}': granted call must start as Ask",
            case.name
        );
        let _ = apply_ask_choice(
            granted,
            AskChoice::AllowAlways,
            &mut cache,
            case.tool,
            "closure",
            &case.granted_args,
        );

        let checked = parsed.evaluate(case.tool, &case.checked_args);
        let overridden =
            apply_session_grant_override(checked, &cache, case.tool, "closure", &case.checked_args);
        assert_eq!(
            overridden.action,
            PermissionAction::Allow,
            "case '{}' must inherit the session grant across mode values",
            case.name
        );
    }
}
