use super::*;

#[test]
fn parse_from_yaml_valid_table() {
    // -- Setup & Fixtures
    type Case = (
        &'static str,
        &'static str,
        Option<PermissionAction>,
        Vec<(&'static str, PermissionAction)>,
    );

    let cases: Vec<Case> = vec![
        (
            "global allow",
            r#"
        "*": "allow"
        "#,
            Some(PermissionAction::Allow),
            vec![],
        ),
        (
            "tool rules",
            r#"
        read: "allow"
        shell: "deny"
        "#,
            None,
            vec![
                ("read", PermissionAction::Allow),
                ("shell", PermissionAction::Deny),
            ],
        ),
        ("empty mapping", "", None, vec![]),
    ];

    // -- Exec & Check
    for (name, yaml_src, expected_global, expected_rules) in cases {
        let yaml = if yaml_src.is_empty() {
            noyalib::Mapping::new()
        } else {
            noyalib::from_str::<noyalib::Mapping>(yaml_src).expect("valid YAML")
        };
        let overlay = PermissionsOverlay::parse_from_yaml(&yaml).expect("parse");
        assert_eq!(overlay.global, expected_global, "case '{name}': global");
        assert_eq!(
            overlay.tool_rules.len(),
            expected_rules.len(),
            "case '{name}': tool rule count"
        );
        for (tool, action) in expected_rules {
            assert_eq!(
                overlay.tool_rules.get(tool),
                Some(&action),
                "case '{name}': tool rule for {tool}"
            );
        }
    }
}

#[test]
fn parse_from_yaml_invalid_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &[&str])> = vec![
        (
            "invalid action",
            r#"
        "*": "yolo"
        "#,
            &["permissions.*", "invalid permission action"],
        ),
        (
            "invalid structure",
            r#"
        shell: 42
        "#,
            &["permissions.shell"],
        ),
    ];

    // -- Exec & Check
    for (name, yaml_src, expected_fragments) in cases {
        let yaml = noyalib::from_str::<noyalib::Mapping>(yaml_src).expect("valid YAML");
        let err =
            PermissionsOverlay::parse_from_yaml(&yaml).expect_err("invalid YAML overlay must fail");
        for fragment in expected_fragments {
            assert!(
                err.contains(fragment),
                "case '{name}' must name '{fragment}', got: {err}"
            );
        }
    }
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
