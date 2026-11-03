//! The call-line table: a tool name plus raw arguments, and the one line a
//! surface draws for it.
//!
//! A tailored line for the tools that have one; the generic JSON summary for
//! every other name and for arguments that do not carry the field the tailored
//! line reads.

use crate::call_line::render_for;
use agnt_core::protocol::events::ToolCall;
use agnt_core::tools::handler::ToolSource;
use agnt_core::transcript::ir::ToolName;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_call_line_render_for_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (
            "read on a path",
            "read",
            r#"{"path":"src/lib.rs"}"#,
            "→ src/lib.rs",
        ),
        (
            "read on a window",
            "read",
            r#"{"path":"a.rs","offset":10,"limit":5}"#,
            "→ a.rs:10-15",
        ),
        ("read without a path", "read", "{}", "→ {}"),
        ("edit", "edit", r#"{"path":"a.rs"}"#, "→ a.rs (diff)"),
        (
            "glob at the root",
            "glob",
            r#"{"pattern":"*.rs"}"#,
            "→ *.rs",
        ),
        (
            "glob in a directory",
            "glob",
            r#"{"pattern":"*.rs","path":"src"}"#,
            "→ src/*.rs",
        ),
        (
            "grep with no location",
            "grep",
            r#"{"pattern":"foo"}"#,
            "→ \"foo\"",
        ),
        (
            "grep with a path",
            "grep",
            r#"{"pattern":"foo","path":"src"}"#,
            "→ \"foo\" src",
        ),
        (
            "grep with a glob",
            "grep",
            r#"{"pattern":"foo","glob":"*.rs"}"#,
            "→ \"foo\" *.rs",
        ),
        (
            "http defaults to GET",
            "http",
            r#"{"url":"https://x.dev"}"#,
            "→ GET https://x.dev",
        ),
        (
            "http with a method",
            "http",
            r#"{"url":"https://x.dev","method":"POST"}"#,
            "→ POST https://x.dev",
        ),
        (
            "nu shows its default timeout",
            "nu",
            r#"{"command":"ls"}"#,
            "⏱ 120s",
        ),
        (
            "nu shows a named timeout",
            "nu",
            r#"{"command":"ls","timeout_seconds":5}"#,
            "⏱ 5s",
        ),
        (
            "an unknown name falls back to the JSON summary",
            "k8s__list_pods",
            r#"{"namespace":"prod"}"#,
            "→ {\"namespace\":\"prod\"}",
        ),
        ("unparseable arguments fall back", "read", "a.rs", "→ a.rs"),
    ];

    // -- Exec & Check
    for (case, name, arguments, expected) in cases {
        let call = ToolCall::new(ToolName(name.to_string()), ToolSource::Builtin, arguments);
        assert_eq!(render_for(&call), expected, "case '{case}'");
    }
    Ok(())
}
