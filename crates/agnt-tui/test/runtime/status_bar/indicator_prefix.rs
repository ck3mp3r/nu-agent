use super::*;

/// The status indicator returns an empty circle when idle and cycles through
/// four spinner frames when busy.
#[test]
fn status_indicator_frames() -> Result<()> {
    // -- Exec & Check: idle.
    assert_eq!(crate::runtime::status_indicator_for_test(None), "○");

    // -- Exec & Check: busy frames cycle and wrap.
    let f = crate::runtime::status_indicator_for_test;
    let cases: [(u128, &str); 5] = [(0, "◐"), (150, "◓"), (300, "◑"), (450, "◒"), (600, "◐")];
    for (millis, expected) in cases {
        assert_eq!(f(Some(millis)), expected, "millis={millis}");
    }
    Ok(())
}

/// The lane-1 prefix is the idle empty circle or the busy spinner, never
/// exceeding the available width, and the busy line carries at least one
/// explicitly styled span.
#[test]
fn lane_1_prefix_contract() -> Result<()> {
    // -- Exec & Check: idle prefix.
    let idle = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel", None, None, 40,
    );
    let idle_text: String = idle.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(idle_text.starts_with("○ mymodel"));

    // -- Exec & Check: busy prefix.
    let busy = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "mymodel",
        None,
        Some(0),
        40,
    );
    let busy_text: String = busy.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(busy_text.starts_with("◐ mymodel"));

    // -- Exec & Check: the prefix never exceeds the available width.
    let narrow = crate::runtime::status::test::compact_status_line_with_branch_for_test(
        "abcdefghijklmnop",
        Some("branchname"),
        None,
        40,
    );
    let narrow_text: String = narrow.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(narrow_text.chars().count() <= 40);
    assert!(narrow_text.starts_with("○ "));

    // -- Exec & Check: the busy line carries at least one styled span.
    let styled = crate::runtime::compact_status_line_for_test("openai/gpt-4o-mini", Some(0));
    assert!(
        styled.spans.iter().any(|s| s.style.fg.is_some()),
        "expected at least one span with explicit fg colour, got: {styled:?}"
    );
    Ok(())
}
