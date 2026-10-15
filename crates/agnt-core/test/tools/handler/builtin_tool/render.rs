use super::*;

// ================================================================
// Call-line rendering: default trait method
// ================================================================

/// A builtin handler that does not override `call_line_render`.
struct PlainRenderTool;

impl super::super::BuiltinTool for PlainRenderTool {
    const NAME: &'static str = "plain_render";

    async fn execute(
        _args: &serde_json::Value,
        _cwd: &Path,
        _bus: &crate::bus::Bus,
    ) -> core::result::Result<serde_json::Value, super::super::super::ToolHandlerError> {
        Ok(serde_json::json!({}))
    }
}

/// A tool with no `call_line_render` override falls back to the generic
/// JSON summary: a call line whose summary is the arrow-prefixed truncated
/// arguments.
#[test]
fn call_line_render_defaults_to_generic_json_summary() -> TestResult<()> {
    // -- Setup & Fixtures
    let arguments = r#"{"path":"a.rs","limit":10}"#;

    // -- Exec
    let render = PlainRenderTool::call_line_render(arguments);

    // -- Check
    assert_eq!(
        render,
        crate::protocol::tool_args::CallLine::from_json_summary(arguments)
    );
    assert!(
        render.summary.starts_with('→'),
        "generic summary must start with the arrow marker, got: {}",
        render.summary
    );
    Ok(())
}

// ================================================================
// ToolRenderRegistry
// ================================================================

/// A registered tool's render fn is looked up by name and produces the
/// tool's tailored `CallLine`; an unknown name falls back to the generic
/// JSON summary.
#[test]
fn tool_render_registry_table() -> TestResult<()> {
    // -- Setup & Fixtures
    let arguments = r#"{"path":"/tmp/f"}"#;
    let mut registry = super::super::ToolRenderRegistry::default();
    registry.register("read", super::super::ReadTool::call_line_render);

    // -- Exec & Check
    assert_eq!(
        registry.render("read", arguments),
        crate::protocol::tool_args::CallLine {
            summary: "→ /tmp/f".to_string(),
        },
        "a registered tool must use its tailored render"
    );
    assert_eq!(
        registry.render("not_registered", arguments),
        crate::protocol::tool_args::CallLine::from_json_summary(arguments),
        "an unknown tool name must fall back to the generic summary"
    );
    Ok(())
}

// ================================================================
// call_line_render_for: name-based dispatch
// ================================================================

/// `call_line_render_for` dispatches by tool name: `nu` and `edit` have
/// tailored renders, every other name (unknown or a builtin without a
/// tailored render) falls back to the generic JSON summary.
#[test]
fn call_line_render_for_table() -> TestResult<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, &str, &str, Option<&str>)> = vec![
        (
            "nu dispatches to the timeout summary",
            "nu",
            r#"{"command":"ls | select name"}"#,
            Some("⏱ 120s"),
        ),
        (
            "edit dispatches to the tailored inline render",
            "edit",
            r#"{"path":"a.rs","operation":{"type":"create","content":"x"}}"#,
            Some("→ a.rs (diff)"),
        ),
        (
            "unknown name falls back to generic",
            "not_a_builtin",
            r#"{"path":"/tmp/f"}"#,
            None,
        ),
        (
            "builtin without a tailored render falls back to generic",
            "tmux_session",
            r#"{"session":"work"}"#,
            None,
        ),
    ];

    // -- Exec & Check
    for (name, tool_name, arguments, expected) in cases {
        let render = super::super::call_line_render_for(tool_name, arguments);
        match expected {
            Some(summary) => assert_eq!(
                render,
                crate::protocol::tool_args::CallLine {
                    summary: summary.to_string(),
                },
                "case '{name}'"
            ),
            None => assert_eq!(
                render,
                crate::protocol::tool_args::CallLine::from_json_summary(arguments),
                "case '{name}'"
            ),
        }
    }
    Ok(())
}
