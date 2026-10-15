//! Render basics and status indicator tests for the TTY renderer.

use super::*;

#[test]
fn user_message_has_user_prefix() {
    let block = markdown_block(MessageRole::User, "hello");
    let out = render(&block);
    assert!(out.starts_with("[user] "), "got: {out}");
    assert!(out.contains("hello"));
}

#[test]
fn assistant_has_no_prefix() {
    let block = markdown_block(MessageRole::Assistant, "hi");
    let out = render(&block);
    assert!(
        !out.contains("[assistant]"),
        "should have no prefix, got: {out}"
    );
    assert!(out.starts_with("hi"), "should start with content");
}

#[test]
fn tool_shows_tool_prefix() {
    let block = tool_block("run", "→ run");
    let out = render(&block);
    assert!(out.starts_with("[tool] "), "got: {out}");
}

#[test]
fn spacer_renders_empty_string() {
    let out = render(&spacer_block());
    assert_eq!(out, "");
}

#[test]
fn done_status_shows_checkmark() {
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Done);
    let out = render(&block);
    assert!(out.contains("✓"), "got: {out}");
}

#[test]
fn unknown_status_shows_question_mark() {
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Unknown);
    let out = render(&block);
    assert!(out.contains("? "), "got: {out}");
}

#[test]
fn in_progress_status_shows_first_spinner_frame() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let out = render_at(&block, 0);

    // -- Check: the first braille spinner frame, not the old static ellipsis.
    assert!(
        out.contains("⠋"),
        "InProgress at now_millis=0 must show `⠋`; got: {out}"
    );
    assert!(
        !out.contains('…'),
        "InProgress must not use the static ellipsis; got: {out}"
    );
    Ok(())
}

#[test]
fn in_progress_status_shows_second_spinner_frame_after_100ms() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let first = render_at(&block, 0);
    let second = render_at(&block, 100);

    // -- Check
    assert!(
        second.contains("⠙"),
        "InProgress at now_millis=100 must show `⠙`; got: {second}"
    );
    assert_ne!(
        first, second,
        "the indicator must animate across 100 ms; both frames were {first:?}"
    );
    Ok(())
}

#[test]
fn in_progress_indicator_matches_item_status_method() -> Result<()> {
    // -- Setup & Fixtures: the TTY indicator is the shared method's output
    // plus the trailing space the format string requires.
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::InProgress);

    // -- Exec
    let out = render_at(&block, 350);

    // -- Check
    let expected = ItemStatus::InProgress.indicator_char(350);
    assert!(
        out.contains(&format!("{expected} ")),
        "TTY indicator must be `{expected} ` (method output + trailing space); got: {out}"
    );
    Ok(())
}
