//! Markdown, notice, and banner layout tests for the TTY renderer.

use super::*;

/// Markdown blocks project raw text lines with the role prefix on row 0.
#[test]
fn layout_user_markdown_has_prefix_and_content() -> Result<()> {
    // -- Setup & Fixtures
    let block = markdown_block(MessageRole::User, "hello there");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.starts_with("[user] "), "got: {out}");
    assert!(out.contains("hello there"), "got: {out}");
    Ok(())
}

/// Notice blocks render their text on the system prefix.
#[test]
fn layout_notice_renders_text() -> Result<()> {
    // -- Setup & Fixtures
    let block = notice_block("compaction done");

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("[system]"), "got: {out}");
    assert!(out.contains("compaction done"), "got: {out}");
    Ok(())
}

/// Banner blocks render their text lines with no prefix.
#[test]
fn layout_banner_renders_text_lines() -> Result<()> {
    // -- Setup & Fixtures
    let block = Block {
        source: BlockSource::Banner {
            text: "banner line".to_string(),
        },
        lane: Lane::SystemBlank,
        fill: Fill::None,
        status: None,
    };

    // -- Exec
    let out = render(&block);

    // -- Check
    assert_eq!(out, "banner line");
    Ok(())
}

/// Status indicators appear on row 0 through layout() as well.
#[test]
fn layout_done_status_shows_checkmark() -> Result<()> {
    // -- Setup & Fixtures
    let mut block = tool_block("t", "→ t");
    block.status = Some(ItemStatus::Done);

    // -- Exec
    let out = render(&block);

    // -- Check
    assert!(out.contains("✓"), "got: {out}");
    Ok(())
}
