use super::*;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn parser_accepts_canonical_permissions_shape_and_normalizes_whitespace() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);

    let deny = parsed.evaluate("shell", &args_map!({"command": "kubectl delete pod x"}));
    assert_eq!(deny.action, PermissionAction::Deny);
    assert_eq!(deny.matched_rule.scope, "nested");
    assert_eq!(deny.matched_rule.target_field, Some("command".to_string()));

    let normalized = parsed.evaluate(
        "shell",
        &args_map!({"command": "   kubectl    delete   pod   foo   "}),
    );
    assert_eq!(normalized.action, PermissionAction::Deny);
}

#[test]
fn precedence_is_global_then_tool_then_nested_command() {
    let parsed = PermissionsConfig::from_toml(&permissions_toml(), true);

    let global = parsed.evaluate("unknown_tool", &args_map!({}));
    assert_eq!(global.action, PermissionAction::Ask);
    assert_eq!(global.matched_rule.scope, "global");

    let tool = parsed.evaluate("read", &args_map!({}));
    assert_eq!(tool.action, PermissionAction::Allow);
    assert_eq!(tool.matched_rule.scope, "tool");

    let nested = parsed.evaluate("shell", &args_map!({"command": "kubectl delete ns prod"}));
    assert_eq!(nested.action, PermissionAction::Deny);
    assert_eq!(nested.matched_rule.scope, "nested");
}

#[test]
fn decision_timeout_secs_table() -> Result<()> {
    // -- Setup & Fixtures
    type Case = (
        &'static str,
        &'static str,
        Option<Duration>,
        Option<&'static str>,
    );

    let cases: Vec<Case> = vec![
        (
            "absent key keeps the 60s default",
            r#"
"*" = "ask"
"#,
            Some(Duration::from_secs(60)),
            None,
        ),
        (
            "explicit seconds",
            r#"
"*" = "ask"
decision_timeout_secs = 45
"#,
            Some(Duration::from_secs(45)),
            None,
        ),
        (
            "zero disables the timeout",
            r#"
"*" = "ask"
decision_timeout_secs = 0
"#,
            None,
            None,
        ),
        (
            "negative seconds fall back to the 60s default",
            r#"
"*" = "ask"
decision_timeout_secs = -5
"#,
            Some(Duration::from_secs(60)),
            Some("permissions.invalid.decision_timeout"),
        ),
        (
            "non-integer seconds fall back to the 60s default",
            r#"
"*" = "ask"
decision_timeout_secs = "soon"
"#,
            Some(Duration::from_secs(60)),
            Some("permissions.invalid.decision_timeout"),
        ),
    ];

    // -- Exec & Check
    for (name, toml_src, expected, expected_diagnostic) in cases {
        let value: toml::Value = toml::from_str(toml_src)?;
        let parsed = PermissionsConfig::from_toml(&value, true);
        assert_eq!(parsed.decision_timeout(), expected, "case '{name}'");
        let decision = parsed.evaluate("unknown_tool", &args_map!({}));
        match expected_diagnostic {
            Some(code) => assert!(
                decision.diagnostics.iter().any(|diag| diag.code == code),
                "case '{name}' must emit diagnostic '{code}', got: {:?}",
                decision.diagnostics
            ),
            None => assert!(
                decision
                    .diagnostics
                    .iter()
                    .all(|diag| diag.code != "permissions.invalid.decision_timeout"),
                "case '{name}' must not emit a decision-timeout diagnostic, got: {:?}",
                decision.diagnostics
            ),
        }
    }

    // -- Check: the key is not mistaken for a tool rule.
    let value: toml::Value = toml::from_str(
        r#"
"*" = "ask"
decision_timeout_secs = 45
"#,
    )?;
    let parsed = PermissionsConfig::from_toml(&value, true);
    assert!(
        parsed
            .evaluate("decision_timeout_secs", &args_map!({}))
            .matched_rule
            .scope
            == "global",
        "the timeout key must not become a tool rule"
    );
    assert!(
        parsed
            .evaluate("unknown_tool", &args_map!({}))
            .diagnostics
            .iter()
            .all(|diag| diag.code != "permissions.invalid.tool_action"),
        "the timeout key must not emit a tool-action diagnostic"
    );
    Ok(())
}

#[test]
fn safe_defaults_carry_the_decision_timeout() {
    // -- Exec & Check
    for interactive in [true, false] {
        let config = PermissionsConfig::safe_defaults(interactive);
        assert_eq!(
            config.decision_timeout(),
            Some(Duration::from_secs(60)),
            "safe defaults must carry the 60s decision timeout (interactive={interactive})"
        );
    }
}

#[test]
fn diagnostics_table() {
    // -- Setup & Fixtures
    let redundant_star: toml::Value = toml::from_str(
        r#"
"*" = "ask"
[shell.command]
"*" = "ask"
"#,
    )
    .expect("valid toml");

    type Case = (
        &'static str,
        &'static str,
        serde_json::Map<String, serde_json::Value>,
        PermissionAction,
        &'static str,
    );

    let cases: Vec<Case> = vec![
        (
            "missing nested field falls back to a safe ask",
            "fixture",
            args_map!({}),
            PermissionAction::Ask,
            "permissions.nested_field.missing",
        ),
        (
            "redundant nested star is a valid no-op",
            "redundant",
            args_map!({"command": "echo hi"}),
            PermissionAction::Ask,
            "permissions.noop.nested_field.star",
        ),
    ];

    // -- Exec & Check
    for (name, toml_kind, args, expected_action, expected_code) in cases {
        let value = if toml_kind == "fixture" {
            permissions_toml()
        } else {
            redundant_star.clone()
        };
        let parsed = PermissionsConfig::from_toml(&value, true);
        let decision = parsed.evaluate("shell", &args);
        assert_eq!(decision.action, expected_action, "case '{name}'");
        assert!(
            decision
                .diagnostics
                .iter()
                .any(|diag| diag.code == expected_code),
            "case '{name}' must emit diagnostic '{expected_code}', got: {:?}",
            decision.diagnostics
        );
    }
}
