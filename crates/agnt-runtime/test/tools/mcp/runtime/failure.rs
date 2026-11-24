use super::*;

/// A multi-line child dump collapses to one row at storage: the surface shows
/// a single line, and the log keeps the dump.
#[test]
fn collapse_failure_reason_flattens_a_multiline_dump() -> Result<()> {
    // -- Setup & Fixtures
    let dump = "failed to connect stdio MCP server: process exited\nusage: stub [--flag]\n\terror: unexpected argument '--injected' found";

    // -- Exec
    let collapsed = crate::tools::mcp::runtime::collapse_failure_reason(dump);

    // -- Check
    assert_eq!(
        collapsed,
        "failed to connect stdio MCP server: process exited usage: stub [--flag] error: unexpected argument '--injected' found"
    );
    assert!(
        !collapsed.chars().any(char::is_control),
        "a collapsed reason must carry no control byte, got: {collapsed:?}"
    );
    Ok(())
}

/// A collapsed reason keeps every word and drops only the layout: no character
/// of the text is lost, and runs of whitespace become one space.
#[test]
fn collapse_failure_reason_keeps_every_word() -> Result<()> {
    // -- Setup & Fixtures
    let spaced = "  timed out    after \u{1b}[1m300ms\u{1b}[0m  ";

    // -- Exec
    let collapsed = crate::tools::mcp::runtime::collapse_failure_reason(spaced);

    // -- Check
    assert_eq!(collapsed, "timed out after [1m300ms [0m");
    assert!(!collapsed.chars().any(char::is_control));
    Ok(())
}
