use super::*;
use nu_protocol::{Value, record};

fn permissions_toml() -> toml::Value {
    toml::from_str(
        r#"
"*" = "ask"
read = "allow"
"c5t_get*" = "allow"

[shell.command]
"kubectl delete *" = "deny"
"*" = "ask"
"#,
    )
    .expect("valid toml")
}

#[test]
fn parser_accepts_canonical_permissions_shape() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let deny = parsed.evaluate(
        "shell",
        &serde_json::json!({"command": "kubectl delete pod x"}),
    );
    assert_eq!(deny.action, PermissionAction::Deny);
    assert_eq!(deny.matched_rule.scope, "nested");
    assert_eq!(deny.matched_rule.target_field, Some("command".to_string()));
}

#[test]
fn precedence_is_global_then_tool_then_nested_command() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);

    let global = parsed.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(global.action, PermissionAction::Ask);
    assert_eq!(global.matched_rule.scope, "global");

    let tool = parsed.evaluate("read", &serde_json::json!({}));
    assert_eq!(tool.action, PermissionAction::Allow);
    assert_eq!(tool.matched_rule.scope, "tool");

    let nested = parsed.evaluate(
        "shell",
        &serde_json::json!({"command": "kubectl delete ns prod"}),
    );
    assert_eq!(nested.action, PermissionAction::Deny);
    assert_eq!(nested.matched_rule.scope, "nested");
}

#[test]
fn command_matching_normalizes_whitespace() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let decision = parsed.evaluate(
        "shell",
        &serde_json::json!({"command": "   kubectl    delete   pod   foo   "}),
    );
    assert_eq!(decision.action, PermissionAction::Deny);
}

#[test]
fn missing_command_uses_deterministic_safe_fallback_with_diagnostics() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let decision = parsed.evaluate("shell", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Ask);
    assert!(
        decision
            .diagnostics
            .iter()
            .any(|diag| diag.code == "permissions.nested_field.missing")
    );
}

#[test]
fn redundant_nested_star_equal_to_inherited_is_valid_noop_with_diagnostic() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
[shell.command]
"*" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let decision = parsed.evaluate("shell", &serde_json::json!({"command": "echo hi"}));

    assert_eq!(decision.action, PermissionAction::Ask);
    assert!(
        decision
            .diagnostics
            .iter()
            .any(|diag| diag.code == "permissions.noop.nested_field.star")
    );
}

#[test]
fn ask_choices_apply_once_always_and_deny() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();
    let args = serde_json::json!({"command": "echo hi"});

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
    let first_args = serde_json::json!({"command": "echo one"});
    let second_args = serde_json::json!({"command": "echo two"});

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

#[test]
fn allow_always_for_shell_does_not_leak_to_read_under_global_ask() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let mut cache = SessionGrantCache::default();

    let shell_args = serde_json::json!({"command": "echo one"});
    let shell_decision = parsed.evaluate("shell", &shell_args);
    let _ = apply_ask_choice(
        shell_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &shell_args,
    );

    let read_args = serde_json::json!({"filePath": "README.md"});
    let read = parsed.evaluate("read", &read_args);
    assert_eq!(read.matched_rule.identity, "global:*");
    let overridden = apply_session_grant_override(read, &cache, "read", "closure", &read_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_rule_identity_different_tool_name_does_not_share_session_grant() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let mut cache = SessionGrantCache::default();

    let glob_args = serde_json::json!({"pattern": "**/*.rs"});
    let glob = parsed.evaluate("glob", &glob_args);
    let _ = apply_ask_choice(
        glob,
        AskChoice::AllowAlways,
        &mut cache,
        "glob",
        "closure",
        &glob_args,
    );

    let read_args = serde_json::json!({"filePath": "README.md"});
    let read = parsed.evaluate("read", &read_args);
    assert_eq!(read.matched_rule.identity, "global:*");
    let overridden = apply_session_grant_override(read, &cache, "read", "closure", &read_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_name_different_mode_does_not_share_session_grant() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"edit" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let mut cache = SessionGrantCache::default();

    let preview_args = serde_json::json!({
        "path": "file.txt",
        "mode": "preview"
    });
    let preview = parsed.evaluate("edit", &preview_args);
    let _ = apply_ask_choice(
        preview,
        AskChoice::AllowAlways,
        &mut cache,
        "edit",
        "closure",
        &preview_args,
    );

    let apply_args = serde_json::json!({
        "path": "file.txt",
        "mode": "apply"
    });
    let apply = parsed.evaluate("edit", &apply_args);
    let overridden = apply_session_grant_override(apply, &cache, "edit", "closure", &apply_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_name_same_mode_different_source_does_not_share_session_grant() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"edit" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let mut cache = SessionGrantCache::default();

    let args = serde_json::json!({
        "path": "file.txt",
        "mode": "apply"
    });
    let closure_decision = parsed.evaluate("edit", &args);
    let _ = apply_ask_choice(
        closure_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "edit",
        "closure",
        &args,
    );

    let mcp_decision = parsed.evaluate("edit", &args);
    let overridden = apply_session_grant_override(mcp_decision, &cache, "edit", "mcp", &args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn same_tool_source_mode_different_rule_identity_does_not_share_session_grant() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
[shell.command]
"echo *" = "ask"
"ls *" = "ask"
"#,
    )
    .expect("valid toml");
    let parsed = PermissionsConfig::from_toml(&value, true);
    let mut cache = SessionGrantCache::default();

    let echo_args = serde_json::json!({"command": "echo one"});
    let echo_decision = parsed.evaluate("shell", &echo_args);
    assert_eq!(
        echo_decision.matched_rule.identity,
        "nested:shell.command:echo *"
    );
    let _ = apply_ask_choice(
        echo_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &echo_args,
    );

    let ls_args = serde_json::json!({"command": "ls -la"});
    let ls_decision = parsed.evaluate("shell", &ls_args);
    assert_eq!(
        ls_decision.matched_rule.identity,
        "nested:shell.command:ls *"
    );
    let overridden =
        apply_session_grant_override(ls_decision, &cache, "shell", "closure", &ls_args);
    assert_eq!(overridden.action, PermissionAction::Ask);
}

#[test]
fn defaults_apply_when_permissions_block_is_missing() {
    let parsed = PermissionsConfig::safe_defaults(true);
    assert_eq!(
        parsed.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Allow
    );
    assert_eq!(
        parsed
            .evaluate("shell", &serde_json::json!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Ask
    );
}

#[test]
fn allow_always_grant_is_session_only_and_resets_with_new_cache() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);

    let mut first_session_cache = SessionGrantCache::default();
    let first_args = serde_json::json!({"command": "echo one"});
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
    let second_args = serde_json::json!({"command": "echo two"});
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
fn cli_permissions_overlay_rejects_non_record_with_explicit_path() {
    let err = PermissionsOverlay::parse_from_cli_value(&Value::test_string("ask"))
        .expect_err("non-record --permissions must fail");
    assert!(err.contains("permissions"));
    assert!(err.contains("record"));
}

#[test]
fn cli_permissions_overlay_rejects_invalid_action_with_explicit_leaf_path() {
    let err = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "read" => Value::test_string("prompt")
    }))
    .expect_err("invalid action must fail");

    assert!(err.contains("permissions.read"));
    assert!(err.contains("invalid permission action"));
}

#[test]
fn additive_overlay_cli_wins_on_overlap_and_retains_non_overlapping() {
    let base = PermissionsConfig::from_toml(
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

    let overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "read" => Value::test_string("deny"),
        "grep" => Value::test_string("allow")
    }))
    .expect("overlay parse");

    let merged = base.with_overlay(&overlay);

    assert_eq!(
        merged.evaluate("read", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged.evaluate("glob", &serde_json::json!({})).action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged.evaluate("grep", &serde_json::json!({})).action,
        PermissionAction::Allow
    );
}

#[test]
fn additive_overlay_merges_nested_command_deterministically() {
    let base = PermissionsConfig::from_toml(
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

    let overlay = PermissionsOverlay::parse_from_cli_value(&Value::test_record(record! {
        "shell" => Value::test_record(record! {
            "command" => Value::test_record(record! {
                "kubectl delete *" => Value::test_string("deny"),
                "*" => Value::test_string("deny")
            })
        })
    }))
    .expect("overlay parse");

    let merged = base.with_overlay(&overlay);

    assert_eq!(
        merged
            .evaluate("shell", &serde_json::json!({"command": "kubectl get pods"}))
            .action,
        PermissionAction::Allow
    );
    assert_eq!(
        merged
            .evaluate(
                "shell",
                &serde_json::json!({"command": "kubectl delete pod x"})
            )
            .action,
        PermissionAction::Deny
    );
    assert_eq!(
        merged
            .evaluate("shell", &serde_json::json!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );

    let merged_again = base.with_overlay(&overlay);
    assert_eq!(
        merged_again
            .evaluate("shell", &serde_json::json!({"command": "echo hi"}))
            .action,
        PermissionAction::Deny
    );
}

#[test]
fn display_tool_name_with_no_args() {
    let result = display_tool_name("skill", &serde_json::json!({}));
    assert_eq!(result, "skill");
}

#[test]
fn display_tool_name_with_single_arg() {
    let result = display_tool_name("skill", &serde_json::json!({"name": "nushell-shell"}));
    assert_eq!(result, "skill(name=nushell-shell)");
}

#[test]
fn display_tool_name_with_multiple_args_sorted_alphabetically() {
    let result = display_tool_name(
        "edit",
        &serde_json::json!({
            "filePath": "src/foo.rs",
            "oldString": "foo",
            "newString": "bar"
        }),
    );
    assert_eq!(
        result,
        "edit(filePath=src/foo.rs, newString=bar, oldString=foo)"
    );
}

#[test]
fn display_tool_name_truncates_long_string_values() {
    let long_value = "a".repeat(70);
    let result = display_tool_name("read", &serde_json::json!({"filePath": long_value}));
    let expected = format!("read(filePath={}…)", "a".repeat(60));
    assert_eq!(result, expected);
}

#[test]
fn display_tool_name_skips_null_values() {
    let result = display_tool_name(
        "edit",
        &serde_json::json!({
            "filePath": "test.rs",
            "mode": null,
            "content": "hello"
        }),
    );
    assert_eq!(result, "edit(content=hello, filePath=test.rs)");
}

#[test]
fn display_tool_name_all_null_args_returns_tool_name_only() {
    let result = display_tool_name(
        "test",
        &serde_json::json!({
            "arg1": null,
            "arg2": null
        }),
    );
    assert_eq!(result, "test");
}

#[test]
fn display_tool_name_shows_nested_object_as_compact_json() {
    let result = display_tool_name(
        "complex",
        &serde_json::json!({
            "config": {"key": "value", "nested": {"deep": true}}
        }),
    );
    assert!(result.starts_with("complex(config="));
    assert!(result.contains(r#"{"key":"value""#));
}

#[test]
fn display_tool_name_truncates_nested_object_json() {
    let long_obj = serde_json::json!({
        "key1": "value1",
        "key2": "value2",
        "key3": "value3",
        "key4": "value4",
        "key5": "value5",
        "key6": "value6"
    });
    let result = display_tool_name("tool", &serde_json::json!({"data": long_obj}));
    assert!(result.starts_with("tool(data="));
    assert!(result.ends_with("…)"));
    // Verify the truncation marker exists
    assert!(result.contains("…"));
}

#[test]
fn display_tool_name_shows_array_as_compact_json() {
    let result = display_tool_name("list", &serde_json::json!({"items": [1, 2, 3]}));
    assert_eq!(result, "list(items=[1,2,3])");
}

#[test]
fn display_tool_name_truncates_long_array() {
    let long_array: Vec<i32> = (0..100).collect();
    let result = display_tool_name("batch", &serde_json::json!({"values": long_array}));
    assert!(result.starts_with("batch(values="));
    assert!(result.ends_with("…)"));
    // Verify the truncation marker exists
    assert!(result.contains("…"));
}

#[test]
fn display_tool_name_shows_boolean_values() {
    let result = display_tool_name(
        "flag",
        &serde_json::json!({"enabled": true, "verbose": false}),
    );
    assert_eq!(result, "flag(enabled=true, verbose=false)");
}

#[test]
fn display_tool_name_shows_number_values() {
    let result = display_tool_name("config", &serde_json::json!({"count": 42, "ratio": 2.72}));
    assert_eq!(result, "config(count=42, ratio=2.72)");
}

#[test]
fn display_tool_name_non_object_args_returns_tool_name_only() {
    assert_eq!(
        display_tool_name("test", &serde_json::json!("string")),
        "test"
    );
    assert_eq!(display_tool_name("test", &serde_json::json!(123)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!(true)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!(null)), "test");
    assert_eq!(display_tool_name("test", &serde_json::json!([])), "test");
}

#[test]
fn parse_from_yaml_global_allow() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "allow"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, Some(PermissionAction::Allow));
    assert!(overlay.tool_rules.is_empty());
}

#[test]
fn parse_from_yaml_tool_rules() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        read: "allow"
        shell: "deny"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, None);
    assert_eq!(overlay.tool_rules.len(), 2);
    assert_eq!(
        overlay.tool_rules.get("read"),
        Some(&PermissionAction::Allow)
    );
    assert_eq!(
        overlay.tool_rules.get("shell"),
        Some(&PermissionAction::Deny)
    );
}

#[test]
fn parse_from_yaml_invalid_action() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "yolo"
        "#,
    )
    .expect("valid YAML");

    let err = PermissionsOverlay::parse_from_yaml(&yaml).expect_err("invalid action should fail");
    assert!(err.contains("permissions.*"));
    assert!(err.contains("invalid permission action"));
}

#[test]
fn parse_from_yaml_invalid_structure() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        shell: 42
        "#,
    )
    .expect("valid YAML");

    let err =
        PermissionsOverlay::parse_from_yaml(&yaml).expect_err("invalid structure should fail");
    assert!(err.contains("permissions.shell"));
}

#[test]
fn parse_from_yaml_empty_mapping() {
    let yaml = noyalib::Mapping::new();

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, None);
    assert!(overlay.tool_rules.is_empty());
}

#[test]
fn parse_from_yaml_mixed() {
    let yaml = noyalib::from_str::<noyalib::Mapping>(
        r#"
        "*": "ask"
        read: "allow"
        write: "deny"
        shell:
          command:
            "kubectl*": "deny"
            "*": "ask"
        "#,
    )
    .expect("valid YAML");

    let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
    assert_eq!(overlay.global, Some(PermissionAction::Ask));
    assert_eq!(overlay.tool_rules.len(), 2);
    assert_eq!(
        overlay.tool_rules.get("read"),
        Some(&PermissionAction::Allow)
    );
    assert_eq!(
        overlay.tool_rules.get("write"),
        Some(&PermissionAction::Deny)
    );
    assert_eq!(overlay.nested_field_rules.len(), 1);
    let cmd_rules = overlay.nested_field_rules.get("shell").unwrap();
    assert_eq!(cmd_rules.len(), 1);
    let patterns = cmd_rules.get("command").unwrap();
    assert_eq!(patterns.len(), 2);
}

// ── is_tool_visible tests ──────────────────────────────────────────────

#[test]
fn is_tool_visible_returns_true_for_global_ask() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(config.is_tool_visible("shell"));
    assert!(config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_true_for_global_allow() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "allow"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(config.is_tool_visible("shell"));
}

#[test]
fn is_tool_visible_returns_false_for_global_deny() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "deny"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(!config.is_tool_visible("shell"));
    assert!(!config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_false_for_tool_level_deny() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
shell = "deny"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(!config.is_tool_visible("shell"));
    assert!(config.is_tool_visible("read"));
}

#[test]
fn is_tool_visible_returns_true_for_granular_deny_with_tool_level_ask() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
[shell.command]
"kubectl delete *" = "deny"
"*" = "ask"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(config.is_tool_visible("shell"));
}

#[test]
fn is_tool_visible_respects_specificity_over_global() {
    let value: toml::Value = toml::from_str(
        r#"
"*" = "deny"
read = "allow"
"#,
    )
    .expect("valid toml");
    let config = PermissionsConfig::from_toml(&value, true);
    assert!(config.is_tool_visible("read"));
    assert!(!config.is_tool_visible("shell"));
    assert!(!config.is_tool_visible("edit"));
}

// ── safe_defaults(interactive) tests ───────────────────────────────────

#[test]
fn safe_defaults_tui_mode_global_ask() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Ask);
}

#[test]
fn safe_defaults_tty_mode_global_deny() {
    let config = PermissionsConfig::safe_defaults(false);
    let decision = config.evaluate("unknown_tool", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Deny);
}

#[test]
fn safe_defaults_tty_mode_still_allows_read_tools() {
    let config = PermissionsConfig::safe_defaults(false);
    assert!(config.is_tool_visible("read"));
    assert!(config.is_tool_visible("glob"));
    assert!(config.is_tool_visible("grep"));
}

#[test]
fn safe_defaults_tty_mode_hides_unknown_tools() {
    let config = PermissionsConfig::safe_defaults(false);
    assert!(!config.is_tool_visible("shell"));
    assert!(!config.is_tool_visible("nu__shell"));
    assert!(!config.is_tool_visible("edit"));
}

#[test]
fn ast_query_tool_permission_defaults_to_allow() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("ast_query", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Allow);
}

#[test]
fn ast_nodes_tool_permission_defaults_to_allow() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("ast_nodes", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Allow);
}

#[test]
fn ast_refs_tool_permission_defaults_to_allow() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("ast_refs", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Allow);
}

#[test]
fn ast_tree_tool_permission_defaults_to_allow() {
    let config = PermissionsConfig::safe_defaults(true);
    let decision = config.evaluate("ast_tree", &serde_json::json!({}));
    assert_eq!(decision.action, PermissionAction::Allow);
}

// ── SessionGrantCache::clear tests ──────────────────────────────────────

#[test]
fn clear_empties_all_grants() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();

    // Insert grants for multiple tools
    let shell_args = serde_json::json!({"command": "echo one"});
    let shell_decision = parsed.evaluate("shell", &shell_args);
    let _ = apply_ask_choice(
        shell_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &shell_args,
    );

    let read_args = serde_json::json!({"filePath": "README.md"});
    let read_decision = parsed.evaluate("read", &read_args);
    let _ = apply_ask_choice(
        read_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "read",
        "closure",
        &read_args,
    );

    let glob_args = serde_json::json!({"pattern": "**/*.rs"});
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
    let args = serde_json::json!({"command": "echo test"});
    let decision = parsed.evaluate("shell", &args);
    assert_eq!(
        apply_session_grant_override(decision, &cache, "shell", "closure", &args).action,
        PermissionAction::Ask
    );
}

// ── SessionGrantCache::clear_for_server tests ─────────────────────────────

#[test]
fn clear_for_server_removes_only_matching_prefix() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();

    // Insert grants for context7__* tools, gh__* tool, and a local tool
    let context7_search_args = serde_json::json!({"query": "test"});
    let context7_search_decision = parsed.evaluate("context7__search", &context7_search_args);
    let _ = apply_ask_choice(
        context7_search_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "context7__search",
        "mcp",
        &context7_search_args,
    );

    let context7_fetch_args = serde_json::json!({"url": "https://example.com"});
    let context7_fetch_decision = parsed.evaluate("context7__fetch", &context7_fetch_args);
    let _ = apply_ask_choice(
        context7_fetch_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "context7__fetch",
        "mcp",
        &context7_fetch_args,
    );

    let gh_list_prs_args = serde_json::json!({});
    let gh_list_prs_decision = parsed.evaluate("gh__list_prs", &gh_list_prs_args);
    let _ = apply_ask_choice(
        gh_list_prs_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "gh__list_prs",
        "mcp",
        &gh_list_prs_args,
    );

    let shell_args = serde_json::json!({"command": "echo test"});
    let shell_decision = parsed.evaluate("shell", &shell_args);
    let _ = apply_ask_choice(
        shell_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "shell",
        "closure",
        &shell_args,
    );

    // Verify grants exist before clear
    let context7_search_check = parsed.evaluate("context7__search", &context7_search_args);
    assert_eq!(
        apply_session_grant_override(
            context7_search_check,
            &cache,
            "context7__search",
            "mcp",
            &context7_search_args,
        )
        .action,
        PermissionAction::Allow
    );

    let context7_fetch_check = parsed.evaluate("context7__fetch", &context7_fetch_args);
    assert_eq!(
        apply_session_grant_override(
            context7_fetch_check,
            &cache,
            "context7__fetch",
            "mcp",
            &context7_fetch_args,
        )
        .action,
        PermissionAction::Allow
    );

    let gh_list_prs_check = parsed.evaluate("gh__list_prs", &gh_list_prs_args);
    assert_eq!(
        apply_session_grant_override(
            gh_list_prs_check,
            &cache,
            "gh__list_prs",
            "mcp",
            &gh_list_prs_args,
        )
        .action,
        PermissionAction::Allow
    );

    let shell_check = parsed.evaluate("shell", &shell_args);
    assert_eq!(
        apply_session_grant_override(shell_check, &cache, "shell", "closure", &shell_args).action,
        PermissionAction::Allow
    );

    // Clear grants for context7 server
    cache.clear_for_server("context7");

    // Verify context7__* grants are removed
    let context7_search_after = parsed.evaluate("context7__search", &context7_search_args);
    assert_eq!(
        apply_session_grant_override(
            context7_search_after,
            &cache,
            "context7__search",
            "mcp",
            &context7_search_args,
        )
        .action,
        PermissionAction::Ask
    );

    let context7_fetch_after = parsed.evaluate("context7__fetch", &context7_fetch_args);
    assert_eq!(
        apply_session_grant_override(
            context7_fetch_after,
            &cache,
            "context7__fetch",
            "mcp",
            &context7_fetch_args,
        )
        .action,
        PermissionAction::Ask
    );

    // Verify gh__list_prs and shell remain
    let gh_list_prs_after = parsed.evaluate("gh__list_prs", &gh_list_prs_args);
    assert_eq!(
        apply_session_grant_override(
            gh_list_prs_after,
            &cache,
            "gh__list_prs",
            "mcp",
            &gh_list_prs_args,
        )
        .action,
        PermissionAction::Allow
    );

    let shell_after = parsed.evaluate("shell", &shell_args);
    assert_eq!(
        apply_session_grant_override(shell_after, &cache, "shell", "closure", &shell_args).action,
        PermissionAction::Allow
    );
}

#[test]
fn clear_for_server_with_no_matching_grants() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();

    // Insert grants for gh__* tools only
    let gh_list_prs_args = serde_json::json!({});
    let gh_list_prs_decision = parsed.evaluate("gh__list_prs", &gh_list_prs_args);
    let _ = apply_ask_choice(
        gh_list_prs_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "gh__list_prs",
        "mcp",
        &gh_list_prs_args,
    );

    let gh_get_pr_args = serde_json::json!({"number": 1});
    let gh_get_pr_decision = parsed.evaluate("gh__get_pr", &gh_get_pr_args);
    let _ = apply_ask_choice(
        gh_get_pr_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "gh__get_pr",
        "mcp",
        &gh_get_pr_args,
    );

    // Clear grants for context7 (no matching grants)
    cache.clear_for_server("context7");

    // Verify gh__* grants remain
    let gh_list_prs_after = parsed.evaluate("gh__list_prs", &gh_list_prs_args);
    assert_eq!(
        apply_session_grant_override(
            gh_list_prs_after,
            &cache,
            "gh__list_prs",
            "mcp",
            &gh_list_prs_args,
        )
        .action,
        PermissionAction::Allow
    );

    let gh_get_pr_after = parsed.evaluate("gh__get_pr", &gh_get_pr_args);
    assert_eq!(
        apply_session_grant_override(
            gh_get_pr_after,
            &cache,
            "gh__get_pr",
            "mcp",
            &gh_get_pr_args,
        )
        .action,
        PermissionAction::Allow
    );
}

#[test]
fn clear_for_server_empty_cache() {
    let mut cache = SessionGrantCache::default();

    // Should not panic
    cache.clear_for_server("context7");

    // Verify still empty
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let args = serde_json::json!({"command": "echo test"});
    let decision = parsed.evaluate("shell", &args);
    assert_eq!(
        apply_session_grant_override(decision, &cache, "shell", "closure", &args).action,
        PermissionAction::Ask
    );
}

#[test]
fn clear_for_server_exact_prefix_match_only() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);
    let mut cache = SessionGrantCache::default();

    // Insert context7__search and context7x__search
    let context7_search_args = serde_json::json!({"query": "test"});
    let context7_search_decision = parsed.evaluate("context7__search", &context7_search_args);
    let _ = apply_ask_choice(
        context7_search_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "context7__search",
        "mcp",
        &context7_search_args,
    );

    let context7x_search_args = serde_json::json!({"query": "other"});
    let context7x_search_decision = parsed.evaluate("context7x__search", &context7x_search_args);
    let _ = apply_ask_choice(
        context7x_search_decision,
        AskChoice::AllowAlways,
        &mut cache,
        "context7x__search",
        "mcp",
        &context7x_search_args,
    );

    // Clear grants for context7 (prefix is "context7__")
    cache.clear_for_server("context7");

    // Verify context7__search is removed
    let context7_search_after = parsed.evaluate("context7__search", &context7_search_args);
    assert_eq!(
        apply_session_grant_override(
            context7_search_after,
            &cache,
            "context7__search",
            "mcp",
            &context7_search_args,
        )
        .action,
        PermissionAction::Ask
    );

    // Verify context7x__search remains (does not start with "context7__")
    let context7x_search_after = parsed.evaluate("context7x__search", &context7x_search_args);
    assert_eq!(
        apply_session_grant_override(
            context7x_search_after,
            &cache,
            "context7x__search",
            "mcp",
            &context7x_search_args,
        )
        .action,
        PermissionAction::Allow
    );
}
