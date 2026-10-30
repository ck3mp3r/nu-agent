//! The input caret as a buffer cell: where it lands, the glyph it paints per
//! input mode, and when it is skipped.

use super::*;

use ratatui::layout::Position;
use ratatui::style::Modifier;

/// WHEN a frame renders, THE terminal cursor SHALL stay hidden and THE input
/// caret SHALL paint a buffer cell: on the second visual row of a wrapped line,
/// on the row after an alt-return, and on the last visible row of an internally
/// scrolled input.
#[tokio::test]
async fn caret_cell_positions_on_visual_row() -> Result<()> {
    // -- Setup & Fixtures: a wrapped line.
    let mut wrapped = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();

    // -- Exec
    wrapped.advance_with_frame(&script).await?;

    // -- Check: no terminal cursor, and the caret paints the bar on the second
    // visual row, three cells into the wrapped tail.
    assert!(
        !wrapped.cursor_visible(),
        "the render path must hide the terminal cursor"
    );
    let indicator_y = indicator_row(&wrapped)?;
    let text_x = text_start_x(&wrapped, indicator_y)?;
    let caret = caret_cell(&wrapped)?;
    assert_eq!(
        caret.position,
        Position::new(text_x + 3, indicator_y + 1),
        "the caret cell must sit on the wrapped line's second visual row"
    );
    assert_eq!(
        caret.style.fg,
        wrapped.coordinator().theme.input_text.fg,
        "the insert-mode caret bar must carry the input text foreground"
    );
    assert!(
        !caret.style.add_modifier.contains(Modifier::REVERSED),
        "the insert-mode caret bar must not reverse the cell"
    );

    // -- Setup & Fixtures: a wrapping line plus AltEnter.
    let mut alt = RenderLoopDriver::new(20, 30);
    let mut script: Vec<DriveEvent> = "hello world foo"
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    script.push(key(TerminalKey::AltEnter));

    // -- Exec
    alt.advance_with_frame(&script).await?;

    // -- Check: the new logical line starts at visual row 2.
    let indicator_y = indicator_row(&alt)?;
    let text_x = text_start_x(&alt, indicator_y)?;
    let caret = caret_cell(&alt)?;
    assert_eq!(
        caret.position,
        Position::new(text_x, indicator_y + 2),
        "the caret cell must sit on the new logical line's visual row"
    );

    // -- Setup & Fixtures: a long single word that wraps to >6 visual rows.
    let mut scrolled = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "x"
        .repeat(100)
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();

    // -- Exec
    scrolled.advance_with_frame(&script).await?;

    // -- Check: the caret cell is on the last visible row.
    let indicator_y = indicator_row(&scrolled)?;
    let text_x = text_start_x(&scrolled, indicator_y)?;
    let caret = caret_cell(&scrolled)?;
    assert_eq!(
        caret.position,
        Position::new(text_x + 2, indicator_y + 5),
        "the caret cell must sit on the input's last visible row"
    );
    Ok(())
}

/// WHEN the input mode switches from Insert to Normal, THE caret cell SHALL
/// change from the bar glyph to the character under the caret in a reversed
/// style, and THE cell the caret leaves SHALL return to its plain text glyph.
#[tokio::test]
async fn caret_cell_switches_glyph_with_mode_and_restores_the_previous_cell() -> Result<()> {
    // -- Setup & Fixtures: three characters typed, caret after the last one.
    let mut driver = RenderLoopDriver::new(20, 30);
    let script: Vec<DriveEvent> = "abc".chars().map(|c| key(TerminalKey::Char(c))).collect();
    driver.advance_with_frame(&script).await?;
    let indicator_y = indicator_row(&driver)?;
    let text_x = text_start_x(&driver, indicator_y)?;
    let insert_caret = caret_cell(&driver)?;
    assert_eq!(
        insert_caret.position,
        Position::new(text_x + 3, indicator_y),
        "the insert-mode caret cell must trail the typed characters"
    );
    assert_eq!(insert_caret.symbol, CARET_BAR);

    // -- Exec: step the caret back one character, then leave insert mode.
    driver
        .advance_with_frame(&[key(TerminalKey::Left), key(TerminalKey::Esc)])
        .await?;

    // -- Check: the caret is the character it sits on, reversed, and the cell the
    // bar left behind is blank again.
    let caret = driver
        .reversed_cell()
        .ok_or("normal mode must paint the caret cell in a reversed style")?;
    assert_eq!(
        caret.position,
        Position::new(text_x + 2, indicator_y),
        "the normal-mode caret cell must sit on the character before the old position"
    );
    assert_eq!(
        caret.symbol, "c",
        "the normal-mode caret must keep the character it sits on"
    );
    assert!(
        caret.style.add_modifier.contains(Modifier::REVERSED),
        "the normal-mode caret must read as a reversed block"
    );
    assert_eq!(
        driver.cell_at(text_x + 3, indicator_y).symbol,
        " ",
        "the cell the bar left must return to its plain text glyph"
    );
    assert!(
        driver.cell_with_symbol(CARET_BAR).is_none(),
        "the bar glyph must not survive the switch to normal mode"
    );
    Ok(())
}

/// WHEN the input is locked or a popup is open, THE caret cell SHALL NOT render,
/// and THE terminal cursor SHALL stay hidden.
#[tokio::test]
async fn caret_cell_skipped_when_the_input_is_gated() -> Result<()> {
    // -- Setup & Fixtures: an open info panel.
    let mut panelled = RenderLoopDriver::new(120, 30);
    panelled
        .coordinator_mut()
        .state
        .open_info_panel(crate::state::InfoPanel::Status);

    // -- Exec
    panelled.advance_with_frame(&[]).await?;

    // -- Check
    assert!(
        panelled.cell_with_symbol(CARET_BAR).is_none(),
        "an open info panel must not paint the caret cell"
    );

    // -- Setup & Fixtures: a locked input.
    let mut locked = RenderLoopDriver::new(120, 30);
    locked.coordinator_mut().state.input_locked = true;

    // -- Exec
    locked.advance_with_frame(&[]).await?;

    // -- Check
    assert!(
        locked.cell_with_symbol(CARET_BAR).is_none(),
        "a locked input must not paint the caret cell"
    );
    assert!(
        !locked.cursor_visible(),
        "the terminal cursor must stay hidden behind the gate"
    );
    Ok(())
}

/// WHEN the textarea is replaced by a restored draft, THE caret cell SHALL land
/// on the replaced buffer's own screen row instead of a stale scroll offset.
#[tokio::test]
async fn caret_cell_resets_after_textarea_replacement() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(20, 30);

    // -- Exec: scroll first by typing a long single word that wraps to >6
    // visual rows, so the widget's own scroll offset becomes nonzero.
    let script: Vec<DriveEvent> = "x"
        .repeat(100)
        .chars()
        .map(|c| key(TerminalKey::Char(c)))
        .collect();
    driver.advance_with_frame(&script).await?;
    assert!(
        driver.coordinator().textarea.scroll_offset().0 > 0,
        "scrolled frame must leave the widget scrolled below the first visual row"
    );

    // Replace the textarea via cancel-restore with a shorter buffer whose
    // caret lands at screen row 2 (0 < 2 < height). The next terminal event
    // triggers pickup_restored_input_text, which replaces the textarea.
    driver.coordinator_mut().state.input.restored_input_text =
        Some("hello world foo bar baz qux".to_string());
    driver
        .advance_with_frame(&[key(TerminalKey::Char('z'))])
        .await?;

    // -- Check: the caret cell must sit on row 2, not row 2 minus the stale
    // scroll top.
    let indicator_y = indicator_row(&driver)?;
    let caret = caret_cell(&driver)?;
    assert_eq!(
        caret.position.y,
        indicator_y + 2,
        "the replaced textarea must paint the caret on its own screen row, not a stale scroll offset"
    );
    Ok(())
}
