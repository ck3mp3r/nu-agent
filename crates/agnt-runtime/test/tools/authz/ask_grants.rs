use super::*;

#[test]
fn ask_choices_apply_once_always_and_deny() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();
    let args = args_map!({"command": "echo hi"});

    let base = parsed.evaluate("shell", &args);
    assert_eq!(base.action, PermissionAction::Ask);

    let once = apply_ask_choice(
        base.clone(),
        AskChoice::AllowOnce,
        &mut cache,
        "shell",
        "closure",
        &args,
    );
    assert_eq!(once.action, PermissionAction::Allow);
    assert!(cache.get(&base, "shell", "closure", &args).is_none());

    let always = apply_ask_choice(
        base.clone(),
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &args,
    );
    assert_eq!(always.action, PermissionAction::Allow);
    assert_eq!(
        cache.get(&base, "shell", "closure", &args),
        Some(PermissionAction::Allow)
    );

    let denied = apply_ask_choice(
        base.clone(),
        AskChoice::Deny,
        &mut cache,
        "shell",
        "closure",
        &args,
    );
    assert_eq!(denied.action, PermissionAction::Deny);
}

#[test]
fn session_grants_are_keyed_by_scoped_request_context_not_call_arguments() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();
    let first_args = args_map!({"command": "echo one"});
    let second_args = args_map!({"command": "echo two"});

    let first = parsed.evaluate("shell", &first_args);
    let second = parsed.evaluate("shell", &second_args);
    assert_eq!(first.matched_rule.identity, second.matched_rule.identity);

    let _ = apply_ask_choice(
        first.clone(),
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &first_args,
    );

    let overridden = apply_session_grant_override(second, &cache, "shell", "closure", &second_args);
    assert_eq!(overridden.action, PermissionAction::Allow);
}

/// A session grant is scoped by (tool name, source, matched rule identity).
/// Every axis must isolate the grant: a grant on one axis must not leak into a
/// request that differs on any other axis.
#[test]
fn session_grant_scope_isolation_table() {
    // -- Setup & Fixtures
    struct Case {
        name: &'static str,
        toml: &'static str,
        granted_tool: &'static str,
        granted_source: &'static str,
        granted_args: serde_json::Map<String, serde_json::Value>,
        granted_identity: &'static str,
        checked_tool: &'static str,
        checked_source: &'static str,
        checked_args: serde_json::Map<String, serde_json::Value>,
        checked_identity: &'static str,
    }

    let cases = vec![
        Case {
            name: "global ask does not leak a shell grant to read",
            toml: r#"
"*" = "ask"
"#,
            granted_tool: "shell",
            granted_source: "closure",
            granted_args: args_map!({"command": "echo one"}),
            granted_identity: "global:*",
            checked_tool: "read",
            checked_source: "closure",
            checked_args: args_map!({"filePath": "README.md"}),
            checked_identity: "global:*",
        },
        Case {
            name: "same rule identity but a different tool name",
            toml: r#"
"*" = "ask"
"#,
            granted_tool: "glob",
            granted_source: "closure",
            granted_args: args_map!({"pattern": "**/*.rs"}),
            granted_identity: "global:*",
            checked_tool: "read",
            checked_source: "closure",
            checked_args: args_map!({"filePath": "README.md"}),
            checked_identity: "global:*",
        },
        Case {
            name: "same tool name and mode but a different source",
            toml: r#"
"*" = "ask"
"edit" = "ask"
"#,
            granted_tool: "edit",
            granted_source: "closure",
            granted_args: args_map!({"path": "file.txt", "mode": "apply"}),
            granted_identity: "tool:edit",
            checked_tool: "edit",
            checked_source: "mcp",
            checked_args: args_map!({"path": "file.txt", "mode": "apply"}),
            checked_identity: "tool:edit",
        },
        Case {
            name: "same tool, source and mode but a different rule identity",
            toml: r#"
"*" = "ask"
[shell.command]
"echo *" = "ask"
"ls *" = "ask"
"#,
            granted_tool: "shell",
            granted_source: "closure",
            granted_args: args_map!({"command": "echo one"}),
            granted_identity: "nested:shell.command:echo *",
            checked_tool: "shell",
            checked_source: "closure",
            checked_args: args_map!({"command": "ls -la"}),
            checked_identity: "nested:shell.command:ls *",
        },
    ];

    // -- Exec & Check
    for case in cases {
        let value: toml::Value = toml::from_str(case.toml).expect("valid toml");
        let parsed = PermissionsConfig::from_toml(&value, true);
        let mut cache = SessionGrantCache::default();

        let granted = parsed.evaluate(case.granted_tool, &case.granted_args);
        assert_eq!(
            granted.matched_rule.identity, case.granted_identity,
            "case '{}': granted rule identity",
            case.name
        );
        let _ = apply_ask_choice(
            granted,
            AskChoice::AllowAlways,
            &mut cache,
            case.granted_tool,
            case.granted_source,
            &case.granted_args,
        );

        let checked = parsed.evaluate(case.checked_tool, &case.checked_args);
        assert_eq!(
            checked.matched_rule.identity, case.checked_identity,
            "case '{}': checked rule identity",
            case.name
        );
        let overridden = apply_session_grant_override(
            checked,
            &cache,
            case.checked_tool,
            case.checked_source,
            &case.checked_args,
        );
        assert_eq!(
            overridden.action,
            PermissionAction::Ask,
            "case '{}' must not inherit the session grant",
            case.name
        );
    }
}
