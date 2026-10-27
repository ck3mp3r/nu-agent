//! Input box chrome: border glyphs, focus styling, and theme repaint.

use super::bottom_box_support::*;
use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::rendering::theme::TuiTheme;
use crate::runtime::test_driver::DriveEvent;
use crate::state::{PaneFocus, SwitchRequest};

/// WHEN a frame renders, THE bottom rect SHALL contain no `├` or `┤` divider
/// glyphs, and THE input box SHALL keep its rounded border glyphs.
#[tokio::test]
async fn test_bottom_box_has_no_divider_glyphs_and_keeps_input_border() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let buffer = driver.buffer_text();
    assert!(
        !buffer.contains('├'),
        "bottom box must not paint a ├ divider\n{buffer}"
    );
    assert!(
        !buffer.contains('┤'),
        "bottom box must not paint a ┤ divider\n{buffer}"
    );
    for glyph in ['╭', '╮', '╰', '╯', '│'] {
        assert!(
            buffer.contains(glyph),
            "input box must keep its {glyph} border glyph\n{buffer}"
        );
    }
    Ok(())
}

/// WHEN the pane focus is Input, THE input box border SHALL carry
/// `theme.focus`; WHEN it is Transcript, `theme.subtle_meta`. THE mode
/// indicator SHALL stay `theme.subtle_meta`.
#[tokio::test]
async fn test_bottom_box_input_border_follows_pane_focus_and_indicator_stays_subtle() -> Result<()>
{
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: the default focus is Input.
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let theme = driver.coordinator().theme.clone();
    let top = input_top_row(&driver)?;
    assert_eq!(
        driver.cell_style_at(1, top).fg,
        theme.focus.fg,
        "focused input border must use theme.focus"
    );
    let indicator_row = driver
        .buffer_text()
        .lines()
        .position(|line| line.contains('❯'))
        .ok_or("should find the mode indicator row")? as u16;
    assert_eq!(
        driver.cell_style_at(2, indicator_row).fg,
        theme.subtle_meta.fg,
        "mode indicator must stay theme.subtle_meta"
    );

    // -- Exec: move focus to the transcript.
    driver.coordinator_mut().state.scroll.pane_focus = PaneFocus::Transcript;
    driver.coordinator_mut().mark_render_needed();
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let top = input_top_row(&driver)?;
    assert_eq!(
        driver.cell_style_at(1, top).fg,
        theme.subtle_meta.fg,
        "unfocused input border must use theme.subtle_meta"
    );
    Ok(())
}

/// WHEN the theme switches to CatppuccinLatte mid-session, THE status bar and
/// the stack gap row SHALL repaint with the latte shades on the next frame.
#[tokio::test]
async fn test_bottom_box_theme_switch_repaints_status_and_gap() -> Result<()> {
    // -- Setup & Fixtures: one prompt in progress and one queued, so the stack
    // rows and the gap row are painted.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("active-prompt".to_string());
    let _ = driver.coordinator_mut().state.activate_next_prompt();
    driver
        .coordinator_mut()
        .state
        .enqueue_prompt("queued-under-mocha".to_string());
    let mocha = TuiTheme::catppuccin_mocha();
    let latte = TuiTheme::catppuccin_latte();
    assert_ne!(
        mocha.base, latte.base,
        "fixture requires the two themes to differ on the base"
    );

    // -- Exec: render the first frame under mocha.
    driver.advance_with_frame(&[]).await?;

    // -- Check: mocha shades with the queued prompt still stacked.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let stack_row = lines
        .iter()
        .position(|line| line.contains("▏ queued-under-mocha"))
        .ok_or("the queued prompt must paint a stack row")? as u16;
    let top = input_top_row(&driver)?;
    let input_h = driver.coordinator().input_height;
    let padding_bottom = stack_row + 1;
    let gap = stack_row - 2;
    assert_eq!(
        top,
        padding_bottom + 1,
        "the bottom padding row must hug the input box top border"
    );
    let status_y = top + 1 + input_h + 1;
    assert_eq!(
        cell_bg(&driver, 5, gap),
        Some(mocha.base),
        "gap row must paint the mocha base before the switch"
    );
    assert_eq!(
        cell_bg(&driver, 5, status_y),
        Some(mocha.base),
        "status row must paint the mocha base before the switch"
    );

    // -- Exec: queue the switch and deliver a key so the loop pumps it.
    driver
        .coordinator_mut()
        .state
        .queue_switch_request(SwitchRequest::Theme("CatppuccinLatte".to_string()));
    driver
        .advance_with_frame(&[DriveEvent::Key(TerminalEvent::Key(TerminalKey::Char('x')))])
        .await?;

    // -- Check: latte shades, distinct from the mocha values, with the queued
    // prompt still stacked.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let stack_row = lines
        .iter()
        .position(|line| line.contains("▏ queued-under-mocha"))
        .ok_or("the queued prompt must stay stacked after the switch")? as u16;
    let top = input_top_row(&driver)?;
    let input_h = driver.coordinator().input_height;
    let padding_bottom = stack_row + 1;
    let gap = stack_row - 2;
    assert_eq!(
        top,
        padding_bottom + 1,
        "the bottom padding row must still hug the input box top border"
    );
    let status_y = top + 1 + input_h + 1;
    assert_eq!(
        cell_bg(&driver, 5, stack_row),
        Some(latte.row_user_bg),
        "stack row must repaint with the latte user-turn shade"
    );
    assert_eq!(
        cell_bg(&driver, 5, gap),
        Some(latte.base),
        "gap row must repaint with the latte base after the switch"
    );
    assert_ne!(
        cell_bg(&driver, 5, gap),
        Some(mocha.base),
        "gap row must not keep the mocha base after the switch"
    );
    assert_eq!(
        cell_bg(&driver, 5, status_y),
        Some(latte.base),
        "status row must repaint with the latte base after the switch"
    );
    assert_ne!(
        cell_bg(&driver, 5, status_y),
        Some(mocha.base),
        "status row must not keep the mocha base after the switch"
    );
    Ok(())
}
