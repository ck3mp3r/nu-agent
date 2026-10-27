//! Queued-message stack rendering: position, wrapping, rail, and height.

use agnt_core::transcript::ir::{Block as IrBlock, MessageRole};
use agnt_core::transcript::items::Message;
use agnt_core::transcript::renderer::{FrameContext, Renderable};

use super::bottom_box_support::*;
use crate::rendering::theme::TuiTheme;
use crate::runtime::render::bottom_box::stacked_messages_height;
use crate::tui_renderer::layout;

/// WHEN two or more prompts are stacked, THE bottom box SHALL render one
/// `row_user_bg` padding row, the message rows back to back with a continuous
/// `▏` gutter (no blank row between messages), one `row_user_bg` padding row,
/// a `base` gap row above the stack, and THEN the input box top border flush
/// below the bottom padding row.
#[tokio::test]
async fn test_bottom_box_queued_stack_paints_user_rows_gap_and_footer_status() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("first-queued".to_string());
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("second-queued".to_string());

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let theme = driver.coordinator().theme.clone();
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.contains("▏ first-queued"))
        .ok_or("should find the first stack row")? as u16;
    let second = lines
        .iter()
        .position(|line| line.contains("▏ second-queued"))
        .ok_or("should find the second stack row")? as u16;
    assert_eq!(
        second,
        first + 1,
        "stacked messages must sit back to back with no row between them"
    );
    let padding_top = first - 1;
    let padding_bottom = second + 1;
    let gap = padding_top - 1;
    for y in [padding_top, first, second, padding_bottom] {
        assert_eq!(
            cell_bg(&driver, 1, y),
            Some(theme.row_user_bg),
            "stack row must paint row_user_bg at the left edge"
        );
        assert_eq!(
            cell_bg(&driver, 118, y),
            Some(theme.row_user_bg),
            "stack row must paint row_user_bg at the right edge"
        );
    }
    for y in [first, second] {
        assert!(
            lines[y as usize].contains('▏'),
            "every stack text row must carry the user rail\n{buffer}"
        );
    }
    for y in [padding_top, padding_bottom] {
        assert!(
            !lines[y as usize].contains('▏'),
            "the padding rows bracketing the text must be blank\n{buffer}"
        );
    }
    assert_eq!(
        driver.cell_style_at(5, first).fg,
        theme.role_user.fg,
        "stack text must carry the role_user foreground"
    );
    assert_eq!(
        cell_bg(&driver, 5, gap),
        Some(theme.base),
        "a base gap must sit above the stack"
    );
    assert!(
        !lines[gap as usize].contains('▏'),
        "the gap row above the stack must be blank\n{buffer}"
    );
    let top = input_top_row(&driver)?;
    assert_eq!(
        top,
        padding_bottom + 1,
        "the bottom padding row must hug the input box top border"
    );
    let input_h = driver.coordinator().input_height;
    assert_eq!(
        cell_bg(&driver, 5, top + 1),
        Some(theme.base),
        "input interior must paint base"
    );
    let status_y = top + 1 + input_h + 1;
    assert_eq!(
        cell_bg(&driver, 1, status_y),
        Some(theme.base),
        "status row must paint base at the left edge"
    );
    assert_eq!(
        cell_bg(&driver, 118, status_y),
        Some(theme.base),
        "status row must paint base at the right edge"
    );
    Ok(())
}

/// WHEN a queued prompt is wider than the wrap width, THE bottom box SHALL wrap
/// it onto consecutive `row_user_bg` rows, SHALL carry the `▏` rail on every
/// wrapped row, SHALL bracket the text with one `row_user_bg` padding row on
/// each side, and SHALL render every word (no `…` truncation).
#[tokio::test]
async fn test_bottom_box_long_queued_prompt_wraps_with_rail_and_no_truncation() -> Result<()> {
    // -- Setup & Fixtures: 40 short words — far wider than the text budget at
    // 120 terminal columns.
    let mut driver = RenderLoopDriver::new(120, 30);
    let prompt = (0..40)
        .map(|index| format!("word{index} "))
        .collect::<String>();
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt(prompt.trim_end().to_string());

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let theme = driver.coordinator().theme.clone();
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.contains("▏ word0"))
        .ok_or("should find the first wrapped stack row")? as u16;
    let last = lines
        .iter()
        .position(|line| line.contains("word39"))
        .ok_or("the stack must render the last word instead of truncating")? as u16;
    assert!(
        last > first + 1,
        "a long queued prompt must wrap onto more than one row\n{buffer}"
    );
    for y in first..=last {
        assert!(
            lines[y as usize].contains('▏'),
            "every wrapped stack row must carry the rail\n{buffer}"
        );
        assert_eq!(
            cell_bg(&driver, 1, y),
            Some(theme.row_user_bg),
            "every wrapped stack row must paint row_user_bg at the left edge"
        );
        assert_eq!(
            cell_bg(&driver, 118, y),
            Some(theme.row_user_bg),
            "every wrapped stack row must paint row_user_bg at the right edge"
        );
    }
    assert!(
        !lines[first as usize].contains('…'),
        "the stack must not truncate a long prompt\n{buffer}"
    );
    let padding_top = first - 1;
    let padding_bottom = last + 1;
    for y in [padding_top, padding_bottom] {
        assert_eq!(
            cell_bg(&driver, 1, y),
            Some(theme.row_user_bg),
            "a padding row must paint row_user_bg at the left edge"
        );
        assert_eq!(
            cell_bg(&driver, 118, y),
            Some(theme.row_user_bg),
            "a padding row must paint row_user_bg at the right edge"
        );
        assert!(
            !lines[y as usize].contains('▏'),
            "a padding row must be blank\n{buffer}"
        );
    }
    let top = input_top_row(&driver)?;
    assert_eq!(
        top,
        padding_bottom + 1,
        "the bottom padding row must hug the input box top border"
    );
    Ok(())
}

/// WHEN a queued prompt sits below a transcript user message, THE stack text
/// SHALL start at the same column as the transcript user-message text: both
/// wrap at the transcript user-message wrap width.
#[tokio::test]
async fn test_bottom_box_stack_text_aligns_with_transcript_user_message() -> Result<()> {
    // -- Setup & Fixtures: one prompt already active (pushed into the
    // transcript) and one still queued in the stack.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("transcript-user-row".to_string());
    let _ = driver.coordinator_mut().state.activate_next_prompt();
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("stacked-user-row".to_string());

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let transcript_row = lines
        .iter()
        .position(|line| line.contains("transcript-user-row"))
        .ok_or("the active prompt must render in the transcript")?;
    let stack_row = lines
        .iter()
        .position(|line| line.contains("stacked-user-row"))
        .ok_or("the queued prompt must render in the stack")?;
    let transcript_rail = lines[transcript_row]
        .chars()
        .position(|c| c == '▏')
        .ok_or("the transcript user row must paint the rail")?;
    let stack_rail = lines[stack_row]
        .chars()
        .position(|c| c == '▏')
        .ok_or("the stack row must paint the rail")?;
    assert_eq!(
        stack_rail, transcript_rail,
        "the stack must wrap at the transcript user-message width\n{buffer}"
    );
    Ok(())
}

/// THE `stacked_messages_height` function SHALL equal the rows the stack
/// paints — every block's wrapped rows plus the two padding rows — and SHALL
/// return `0` for an empty stack.
#[test]
fn test_stacked_messages_height_matches_rendered_rows_and_blanks() -> Result<()> {
    // -- Setup & Fixtures
    let theme = TuiTheme::catppuccin_mocha();
    let width = 60;
    let long = (0..40)
        .map(|index| format!("word{index} "))
        .collect::<String>();
    let blocks: Vec<IrBlock> = ["short-one", long.trim_end()]
        .iter()
        .map(|text| {
            let message = Message {
                role: MessageRole::User,
                markdown: (*text).to_string(),
            };
            IrBlock {
                source: message.source(),
                lane: message.lane(),
                fill: message.fill(),
                status: None,
            }
        })
        .collect();
    let ctx = FrameContext {
        width,
        now_millis: 0,
        cursor: false,
        selected: false,
    };

    // -- Exec
    let height = stacked_messages_height(&blocks, width, &theme);
    let rendered = blocks
        .iter()
        .map(|block| layout(block, &ctx, &theme).len())
        .sum::<usize>()
        + 2;

    // -- Check
    assert!(
        rendered > blocks.len() + 2,
        "fixture requires the long prompt to wrap, rendered {rendered}"
    );
    assert_eq!(
        usize::from(height),
        rendered,
        "the reserved stack height must match the painted rows"
    );
    assert_eq!(
        stacked_messages_height(&[], width, &theme),
        0,
        "an empty stack must reserve no rows"
    );
    Ok(())
}
