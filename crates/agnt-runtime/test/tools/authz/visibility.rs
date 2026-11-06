use super::*;

// ── is_tool_visible tests ──────────────────────────────────────────────

#[test]
fn is_tool_visible_table() {
    // -- Setup & Fixtures
    type Case = (&'static str, &'static str, Vec<(&'static str, bool)>);

    let cases: Vec<Case> = vec![
        (
            "global ask",
            r#"
"*" = "ask"
"#,
            vec![("shell", true), ("read", true)],
        ),
        (
            "global allow",
            r#"
"*" = "allow"
"#,
            vec![("shell", true)],
        ),
        (
            "global deny",
            r#"
"*" = "deny"
"#,
            vec![("shell", false), ("read", false)],
        ),
        (
            "tool level deny",
            r#"
"*" = "ask"
shell = "deny"
"#,
            vec![("shell", false), ("read", true)],
        ),
        (
            "granular deny with tool level ask",
            r#"
"*" = "ask"
[shell.command]
"kubectl delete *" = "deny"
"*" = "ask"
"#,
            vec![("shell", true)],
        ),
        (
            "specificity over global",
            r#"
"*" = "deny"
read = "allow"
"#,
            vec![("read", true), ("shell", false), ("edit", false)],
        ),
    ];

    // -- Exec & Check
    for (name, toml_src, checks) in cases {
        let value: toml::Value = toml::from_str(toml_src).expect("valid toml");
        let config = PermissionsConfig::from_toml(&value, true);
        for (tool, expected) in checks {
            assert_eq!(
                config.is_tool_visible(tool),
                expected,
                "case '{name}': is_tool_visible({tool})"
            );
        }
    }
}
