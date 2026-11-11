use super::*;

#[test]
fn safe_defaults_mode_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, bool, &str, PermissionAction)> = vec![
        (
            "tui mode global ask",
            true,
            "unknown_tool",
            PermissionAction::Ask,
        ),
        (
            "tty mode global deny",
            false,
            "unknown_tool",
            PermissionAction::Deny,
        ),
    ];

    // -- Exec & Check
    for (name, interactive, tool, expected) in cases {
        let config = PermissionsConfig::safe_defaults(interactive);
        assert_eq!(
            config.evaluate(tool, &args_map!({})).action,
            expected,
            "case '{name}'"
        );
    }

    let tty = PermissionsConfig::safe_defaults(false);
    for tool in ["read", "glob", "grep"] {
        assert!(
            tty.is_tool_visible(tool),
            "tty mode must keep '{tool}' visible"
        );
    }
    for tool in ["shell", "nu__shell", "edit"] {
        assert!(!tty.is_tool_visible(tool), "tty mode must hide '{tool}'");
    }
}

#[test]
fn ast_tools_default_to_allow() {
    let config = PermissionsConfig::safe_defaults(true);
    for tool in ["ast_query", "ast_nodes", "ast_refs", "ast_tree"] {
        assert_eq!(
            config.evaluate(tool, &args_map!({})).action,
            PermissionAction::Allow,
            "'{tool}' must default to allow"
        );
    }
}
