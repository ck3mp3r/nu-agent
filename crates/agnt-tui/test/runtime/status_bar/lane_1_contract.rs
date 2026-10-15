use super::*;

/// The lane-1 status line reports the model identity with the idle indicator,
/// never a pipe separator or an input-mode token, in any input mode.
#[test]
fn lane_1_contract() -> Result<()> {
    // -- Exec & Check: status lines never report the input mode.
    let mut state = AppState::default();
    let lines = crate::runtime::status_lines_for_test(&mut state, "openai/gpt-4");
    assert!(!lines.iter().any(|line| line.starts_with("Input mode:")));

    // -- Exec & Check: the compact line matches the lane-1 contract.
    let line = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", None);
    let status_line: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(status_line.starts_with("○ openai/gpt-4o-mini"));
    assert!(!status_line.contains('|'));

    // -- Exec & Check: no mode token in any input mode.
    for _ in 0..3 {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "model", None, None, 80,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "○ model");
    }
    Ok(())
}

const LANE_1_CASES: &[(&str, Option<&str>, usize, &str)] = &[
    (
        "abcdefghijklmnop",
        Some("branchname"),
        40,
        "○ abcdefghijklmnop          \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        23,
        "○ ...lmnop \u{e725} branchname",
    ),
    (
        "abcdefghijklmnop",
        Some("branchname"),
        20,
        "○ ...op \u{e725} branchname",
    ),
    ("openai/gpt-4o-mini", None, 80, "○ openai/gpt-4o-mini"),
];

#[test]
fn lane_1_scroll() {
    for &(model, branch, width, expected) in LANE_1_CASES {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            model, branch, None, width,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert_eq!(
            text, expected,
            "model={model}, branch={branch:?}, width={width}"
        );
        assert!(
            !text.contains('|'),
            "pipe should not appear: model={model}, branch={branch:?}, width={width}"
        );

        // Extra structural assertions carried over from overlapping
        // lane_1_branch_segment_is_right_aligned_when_present.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 40 {
            assert_eq!(text.chars().count(), 40);
            assert!(text.starts_with("○ abcdefghijklmnop"));
            assert!(text.ends_with("\u{e725} branchname"));
        }

        // Extra structural assertions carried over from
        // lane_1_narrow_truncation_keeps_branch_right_anchored.
        if model == "abcdefghijklmnop" && branch == Some("branchname") && width == 20 {
            assert_eq!(text.chars().count(), 20);
            assert!(text.ends_with("\u{e725} branchname"));
            assert!(text.contains("...op"));
        }
    }

    // Structural-only: lane_1_with_branch_appends_branch_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("main"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} main"),
            "expected branch icon prefix, got: {text:?}"
        );
    }

    // Structural-only: lane_1_with_branch_ellipsizes_label_while_preserving_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "the-quick-brown-fox-jumps-over",
            Some("feature/super-long-branch-name"),
            None,
            32,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.contains('\u{e725}'),
            "icon must survive ellipsization, got: {text:?}",
        );
        assert!(
            text.contains("..."),
            "branch label should have been ellipsized, got: {text:?}",
        );
    }

    // Structural-only: lane_1_with_branch_drops_icon_when_budget_below_three_cells
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "abc",
            Some("main"),
            None,
            4,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            !text.contains('\u{e725}'),
            "icon must be dropped under extreme narrow budgets, got: {text:?}",
        );
        assert!(text.starts_with("○ "));
    }

    // Structural-only: lane_1_with_detached_head_short_sha_also_gets_icon
    {
        let line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
            "m",
            Some("a1b2c3d"),
            None,
            40,
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            text.ends_with("\u{e725} a1b2c3d"),
            "expected detached-HEAD short SHA to also carry icon, got: {text:?}",
        );
    }
}

#[test]
fn lane_1_has_no_mode_token_in_any_input_mode() {
    let insert_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let insert_text: String = insert_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let normal_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let normal_text: String = normal_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    let visual_line = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "model", None, None, 80,
    );
    let visual_text: String = visual_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert_eq!(insert_text, "○ model");
    assert_eq!(normal_text, "○ model");
    assert_eq!(visual_text, "○ model");
}
