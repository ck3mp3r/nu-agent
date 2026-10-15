use super::*;

#[test]
fn defaults_apply_when_permissions_block_is_missing() {
    let parsed = PermissionsConfig::safe_defaults(true);
    assert_eq!(
        parsed.evaluate("read", &args_map!({})).action,
        PermissionAction::Allow
    );
    assert_eq!(
        parsed
            .evaluate("shell", &args_map!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Ask
    );
}

#[test]
fn allow_always_grant_is_session_only_and_resets_with_new_cache() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);

    let mut first_session_cache = SessionGrantCache::default();
    let first_args = args_map!({"command": "echo one"});
    let first = parsed.evaluate("shell", &first_args);
    let _ = apply_ask_choice(
        first.clone(),
        AskChoice::AllowAlways,
        &mut first_session_cache,
        "shell",
        "closure",
        &first_args,
    );
    let first_overridden = apply_session_grant_override(
        first.clone(),
        &first_session_cache,
        "shell",
        "closure",
        &first_args,
    );
    assert_eq!(first_overridden.action, PermissionAction::Allow);

    let fresh_session_cache = SessionGrantCache::default();
    let second_args = args_map!({"command": "echo two"});
    let second = parsed.evaluate("shell", &second_args);
    let second_overridden = apply_session_grant_override(
        second,
        &fresh_session_cache,
        "shell",
        "closure",
        &second_args,
    );
    assert_eq!(second_overridden.action, PermissionAction::Ask);
}

#[test]
fn cli_permissions_overlay_rejects_invalid_input_with_explicit_path() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Value, &str, &str)> = vec![
        (
            "non-record",
            Value::test_string("ask"),
            "permissions",
            "record",
        ),
        (
            "invalid action",
            Value::test_record(record! {
                "read" => Value::test_string("prompt")
            }),
            "permissions.read",
            "invalid permission action",
        ),
    ];

    // -- Exec & Check
    for (name, value, path_fragment, message_fragment) in cases {
        let err = PermissionsOverlay::parse_from_cli_value(&value)
            .expect_err("invalid --permissions input must fail");
        assert!(
            err.contains(path_fragment),
            "case '{name}' must name the path '{path_fragment}', got: {err}"
        );
        assert!(
            err.contains(message_fragment),
            "case '{name}' must name the reason '{message_fragment}', got: {err}"
        );
    }
}

#[test]
fn additive_overlay_merges_flat_and_nested_deterministically() {
    // -- Setup & Fixtures
    let flat_base = PermissionsConfig::from_toml(
        &toml::from_str::<toml::Value>(
            r#"
"*" = "ask"
read = "allow"
glob = "deny"
"#,
        )
        .expect("valid toml"),
        true,
    );
    let flat_overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "read" => Value::test_string("deny"),
        "grep" => Value::test_string("allow")
    }))
    .expect("overlay parse");

    let nested_base = PermissionsConfig::from_toml(
        &toml::from_str::<toml::Value>(
            r#"
"*" = "ask"
[shell.command]
"kubectl get *" = "allow"
"*" = "ask"
"#,
        )
        .expect("valid toml"),
        true,
    );
    let nested_overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "shell" => Value::test_record(record! {
            "command" => Value::test_record(record! {
                "kubectl delete *" => Value::test_string("deny"),
                "*" => Value::test_string("deny")
            })
        })
    }))
    .expect("overlay parse");

    // -- Exec
    let flat = flat_base.with_overlay(&flat_overlay);
    let nested = nested_base.with_overlay(&nested_overlay);
    let nested_again = nested_base.with_overlay(&nested_overlay);

    // -- Check
    assert_eq!(
        flat.evaluate("read", &args_map!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        flat.evaluate("glob", &args_map!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        flat.evaluate("grep", &args_map!({})).action,
        PermissionAction::Allow
    );

    assert_eq!(
        nested
            .evaluate("shell", &args_map!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        nested
            .evaluate("shell", &args_map!({"command": "kubectl delete pod x"}))
            .action,
        PermissionAction::Deny
    );
    assert_eq!(
        nested
            .evaluate("shell", &args_map!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );
    assert_eq!(
        nested_again
            .evaluate("shell", &args_map!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );
}
