//! Bar-paint tests for the frameless bottom box (task 4c9aa0f9).

use ratatui::style::Color;

use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::rendering::theme::TuiTheme;
use crate::runtime::status::test::init_repo_with_branch;
use crate::runtime::test_driver::{DriveEvent, RenderLoopDriver};
use crate::state::{PaneFocus, SwitchRequest};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Support

/// Background of the cell at `(x, y)` in the last rendered frame.
fn cell_bg(driver: &RenderLoopDriver, x: u16, y: u16) -> Option<Color> {
    driver.cell_style_at(x, y).bg
}

/// Row index of the input box top border. A frame with no popup open paints
/// exactly one `╭`.
fn input_top_row(driver: &RenderLoopDriver) -> Result<u16> {
    driver
        .buffer_text()
        .lines()
        .position(|line| line.contains('╭'))
        .map(|row| row as u16)
        .ok_or_else(|| "should find the input box top border row".into())
}

// endregion: --- Support

// region:    --- Tests

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

/// WHEN a frame renders with an empty queue, THE input interior and the status
/// rows SHALL paint `base`, and THE status SHALL sit flush below the input box
/// bottom border.
#[tokio::test]
async fn test_bottom_box_empty_queue_paints_base_interior_and_footer_status_rows() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let theme = driver.coordinator().theme.clone();
    let top = input_top_row(&driver)?;
    let input_h = driver.coordinator().input_height;
    let bottom = top + 1 + input_h;
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    assert!(
        lines[bottom as usize].contains('╰'),
        "input box bottom border must sit below the interior\n{buffer}"
    );
    assert_eq!(
        cell_bg(&driver, 5, top + 1),
        Some(theme.base),
        "input interior must paint base"
    );
    let status_y = bottom + 1;
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
    assert_ne!(
        cell_bg(&driver, 5, top.saturating_sub(1)),
        Some(theme.row_user_bg),
        "an empty queue must not paint a stack row"
    );
    Ok(())
}

/// WHEN the queue holds prompts, THE stack rows SHALL paint `row_user_bg` full
/// width with a `▏` rail and `role_user` text, a `base` gap SHALL follow the
/// stack, and THE input interior and status rows SHALL keep their shades.
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
    assert_eq!(second, first + 1, "stack rows must be consecutive");
    for y in [first, second] {
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
    assert_eq!(
        driver.cell_style_at(3, first).fg,
        theme.role_user.fg,
        "stack text must carry the role_user foreground"
    );
    let gap = second + 1;
    assert_eq!(
        cell_bg(&driver, 5, gap),
        Some(theme.base),
        "a base gap must sit between the stack and the input box"
    );
    let top = input_top_row(&driver)?;
    assert_eq!(top, gap + 1, "input box top border must follow the gap");
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
    let gap = stack_row + 1;
    assert_eq!(
        top,
        gap + 1,
        "the gap row must sit between the stack and the input box"
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
    let gap = stack_row + 1;
    assert_eq!(
        top,
        gap + 1,
        "the gap row must still sit between the stack and the input box"
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

/// WHEN a frame renders, THE status content SHALL align with the input box
/// interior: the left lane starts at the indicator column and the right lane
/// ends one column left of the right frame column.
#[tokio::test]
async fn test_bottom_box_status_content_aligns_with_input_box_interior() -> Result<()> {
    // -- Setup & Fixtures: a repo branch so the right lane renders.
    let temp_dir = tempfile::TempDir::new()?;
    let repo = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    init_repo_with_branch(&repo, "feature/status-align");
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .set_repo_branch_caller_cwd(Some(repo));
    driver
        .coordinator_mut()
        .state
        .status
        .identity
        .active_model_identity = "gpt-5".to_string();

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check: derive the bottom rect from the input box border row.
    let buffer = driver.buffer_text();
    let lines: Vec<&str> = buffer.lines().collect();
    let border_row = lines
        .iter()
        .position(|line| line.contains('╭'))
        .ok_or("should find the input box top border row")?;
    let border = lines[border_row];
    let rect_x = border
        .chars()
        .position(|c| c == '╭')
        .ok_or("should find the left border glyph")? as u16;
    let rect_right = border
        .chars()
        .position(|c| c == '╮')
        .ok_or("should find the right border glyph")? as u16;
    let rect_width = rect_right - rect_x + 1;

    let left_row = lines
        .iter()
        .position(|line| line.contains("gpt-5"))
        .ok_or("should find the left lane row")?;
    let right_row = lines
        .iter()
        .position(|line| line.contains("feature/status-align"))
        .ok_or("should find the right lane row")?;

    let left_first = lines[left_row]
        .chars()
        .position(|c| c != ' ')
        .ok_or("left lane must paint a glyph")? as u16;
    let right_chars: Vec<char> = lines[right_row].chars().collect();
    let right_last_char = right_chars
        .iter()
        .rposition(|c| *c != ' ')
        .ok_or("right lane must paint a glyph")?;
    let right_last_col: usize = right_chars
        .iter()
        .take(right_last_char)
        .map(|c| unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0))
        .sum();

    assert_eq!(
        left_first,
        rect_x + 1,
        "left lane must start at the input box indicator column"
    );
    assert_eq!(
        right_last_col as u16,
        rect_x + rect_width - 2,
        "right lane must end one column left of the input box right frame column"
    );
    Ok(())
}

// endregion: --- Tests
