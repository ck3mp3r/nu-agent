#[test]
fn compose_exposed_tool_name_table() {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("prefixes server key", "gh", "list_prs", "gh__list_prs"),
        (
            "prevents cross server collisions",
            "altgh",
            "list_prs",
            "altgh__list_prs",
        ),
    ];

    // -- Exec & Check
    for (name, server, tool, expected) in cases {
        let exposed = super::compose_exposed_tool_name(server, tool);
        assert_eq!(exposed, expected, "case '{name}': exposed name");
        assert!(
            exposed.contains("__"),
            "case '{name}': must use the reserved delimiter"
        );
    }

    assert_ne!(
        super::compose_exposed_tool_name("gh", "list_prs"),
        super::compose_exposed_tool_name("altgh", "list_prs"),
        "different servers must not collide"
    );
}

#[test]
fn exposed_name_validation_table() {
    // -- Setup & Fixtures
    let mut owners = std::collections::HashMap::new();
    super::register_exposed_name(&mut owners, "gh__list_prs", "gh").expect("first insert");

    // -- Exec & Check
    let err = super::register_exposed_name(&mut owners, "gh__list_prs", "other")
        .expect_err("duplicate should fail");
    assert!(
        err.contains("duplicate exposed MCP tool name 'gh__list_prs'"),
        "unexpected error: {err}"
    );

    let err = super::validate_raw_tool_name("k8s", "list__pods")
        .expect_err("reserved delimiter should fail");
    assert!(
        err.contains("advertised tool 'list__pods' containing reserved delimiter '__'"),
        "unexpected error: {err}"
    );
}
