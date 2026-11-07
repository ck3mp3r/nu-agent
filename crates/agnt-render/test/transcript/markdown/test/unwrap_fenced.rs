use super::*;

/// `unwrap_single_fenced_block` strips one outer fence, trims the result, and
/// preserves everything else verbatim. Each row is one input/expected pair.
#[test]
fn unwrap_single_fenced_block_table() {
    // -- Setup & Fixtures
    let cases = [
        (
            "```\n## Work State\n- bullet\n```",
            "## Work State\n- bullet",
        ),
        ("```markdown\n# Title\n```", "# Title"),
        ("## Work State\n- bullet", "## Work State\n- bullet"),
        ("", ""),
        ("  ", ""),
        ("hello", "hello"),
        ("```\n## Work State", "## Work State"),
        (
            "```\nBefore\n```rust\nlet x = 1;\n```\nAfter\n```",
            "Before\n```rust\nlet x = 1;\n```\nAfter",
        ),
    ];

    // -- Exec & Check
    for (input, expected) in cases {
        assert_eq!(
            unwrap_single_fenced_block(input),
            expected,
            "input {input:?} should unwrap to {expected:?}"
        );
    }
}
