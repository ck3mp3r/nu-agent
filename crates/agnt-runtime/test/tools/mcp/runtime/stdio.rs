#[test]
fn resolve_stdio_cwd_table() {
    // -- Setup & Fixtures
    let base = std::env::temp_dir();
    let caller = base.join("nu-agent-mcp-caller");
    let override_dir = base.join("nu-agent-mcp-override");
    let relative_base = base.join("nu-agent-mcp-relative");
    let relative_caller = relative_base.join("caller");
    let nested = relative_caller.join("workspace").join("project");
    let absolute_base = base.join("nu-agent-mcp-absolute");
    let absolute_caller = absolute_base.join("caller");
    let absolute_override = absolute_base.join("absolute-override");
    std::fs::create_dir_all(&caller).expect("create caller");
    std::fs::create_dir_all(&override_dir).expect("create override");
    std::fs::create_dir_all(&nested).expect("create nested cwd");
    std::fs::create_dir_all(&absolute_caller).expect("create caller cwd");
    std::fs::create_dir_all(&absolute_override).expect("create absolute override");

    let cases: Vec<(&str, &std::path::Path, Option<String>, &std::path::Path)> = vec![
        (
            "prefers a valid override",
            caller.as_path(),
            Some(override_dir.to_string_lossy().to_string()),
            override_dir.as_path(),
        ),
        (
            "uses the caller when no override",
            caller.as_path(),
            None,
            caller.as_path(),
        ),
        (
            "relative override resolves from caller cwd",
            relative_caller.as_path(),
            Some("workspace/project".to_string()),
            nested.as_path(),
        ),
        (
            "absolute override works",
            absolute_caller.as_path(),
            Some(absolute_override.to_string_lossy().to_string()),
            absolute_override.as_path(),
        ),
    ];

    // -- Exec & Check
    for (name, caller_cwd, override_cwd, expected) in cases {
        let resolved =
            super::resolve_stdio_cwd(caller_cwd, override_cwd, "nu").expect("cwd resolve");
        let expected = std::fs::canonicalize(expected).expect("canonical expected");
        assert_eq!(resolved, expected, "case '{name}'");
    }
}

#[test]
fn resolve_stdio_cwd_error_table() {
    // -- Setup & Fixtures
    let caller = std::env::temp_dir().join("nu-agent-mcp-caller-invalid");
    std::fs::create_dir_all(&caller).expect("create caller");
    let bad = caller.join("does-not-exist");

    // -- Exec & Check
    let err = super::resolve_stdio_cwd(
        caller.as_path(),
        Some(bad.to_string_lossy().to_string()),
        "nu",
    )
    .expect_err("invalid override must fail");
    assert!(
        err.contains("invalid stdio cwd override") || err.contains("not a directory"),
        "unexpected error: {err}"
    );

    let err = super::resolve_caller_cwd(None, "nu").expect_err("missing cwd must fail");
    assert!(
        err.contains("caller cwd") || err.contains("missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn merged_stdio_env_overwrites_pwd_for_compatibility() {
    let cwd = std::env::temp_dir().join("nu-agent-effective-cwd");
    let caller = std::env::temp_dir().join("nu-agent-caller-cwd");
    std::fs::create_dir_all(&cwd).expect("create effective cwd");
    std::fs::create_dir_all(&caller).expect("create caller cwd");
    let env = std::collections::HashMap::from([("PWD".to_string(), "/wrong".to_string())]);

    let merged = super::merged_stdio_env_with_pwd(env, cwd.as_path(), caller.as_path());
    assert_eq!(
        merged.get("PWD").map(String::as_str),
        Some(cwd.to_string_lossy().as_ref())
    );
    assert_eq!(
        merged.get("NU_AGENT_CALLER_CWD").map(String::as_str),
        Some(caller.to_string_lossy().as_ref())
    );
    assert_eq!(
        merged.get("NU_AGENT_CALLER_PWD").map(String::as_str),
        Some(caller.to_string_lossy().as_ref())
    );
}
