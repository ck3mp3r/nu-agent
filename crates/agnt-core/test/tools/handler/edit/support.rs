// -- Test Support

pub(super) fn diff_plan() -> crate::tools::fs::core::EditPlan {
    crate::tools::fs::core::EditPlan {
        replacements: 1,
        would_change: true,
        noop: false,
        conflict: false,
        expected_version: "v1".to_string(),
        previous_version: "v1".to_string(),
        new_version: "v2".to_string(),
        previous_bytes: 12,
        new_bytes: 13,
        previous_lines: 1,
        new_lines: 1,
        previous_content: "let x = 1;\n".to_string(),
        new_content: "let x = 42;\n".to_string(),
    }
}
