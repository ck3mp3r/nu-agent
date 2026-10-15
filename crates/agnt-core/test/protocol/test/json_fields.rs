use super::*;

// === Tests: tool_args ===

#[test]
fn nu_command_from_args_table() {
    // -- Setup & Fixtures
    let cases: &[(&str, Option<&str>)] = &[
        (
            r#"{"command":"ls | select name type size"}"#,
            Some("ls | select name type size"),
        ),
        (r#"{"command":"ls\r\n| get name"}"#, Some("ls\n| get name")),
        (r#"{"path":"a.rs"}"#, None),
        (r#"{"command":42}"#, None),
        ("not-json", None),
        (r#"{"command":""}"#, None),
    ];

    // -- Exec & Check
    for (args, expected) in cases {
        assert_eq!(
            super::tool_args::nu_command_from_args(args).as_deref(),
            *expected,
            "args={args}"
        );
    }
}

#[test]
fn parse_json_field_table() -> Result<()> {
    // -- Setup & Fixtures
    let args = r#"{"path":"a.rs","limit":10,"paths":["a.rs","b.rs","c.rs"]}"#;

    // -- Exec & Check
    assert_eq!(
        super::tool_args::parse_json_string_field(args, "path").as_deref(),
        Some("a.rs")
    );
    assert_eq!(
        super::tool_args::parse_json_string_field(r#"{"path":42}"#, "path"),
        None
    );
    assert_eq!(
        super::tool_args::parse_json_usize_field(args, "limit"),
        Some(10)
    );
    assert_eq!(
        super::tool_args::parse_json_usize_field(r#"{"limit":"ten"}"#, "limit"),
        None
    );
    assert_eq!(
        super::tool_args::parse_json_array_len(args, "paths"),
        Some(3)
    );
    assert_eq!(
        super::tool_args::parse_json_array_len(r#"{"paths":"a.rs"}"#, "paths"),
        None
    );
    Ok(())
}
