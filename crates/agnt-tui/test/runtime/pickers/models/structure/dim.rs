//! Model picker dim tests: rows whose context window cannot hold the
//! session's consumed tokens render dimmed.

use super::support::*;
use super::*;

/// A row whose context window cannot hold the session's consumed tokens is
/// dimmed; a fitting row, a row with an unknown window, and the active row are
/// not.
///
/// The dim is a row style, so the table's highlight style patches the focused
/// row's foreground over it: a focused dimmed row reads normal. The dim shows
/// on unfocused rows, and picking a dimmed row still hits the runtime gate's
/// error.
#[tokio::test]
async fn model_picker_dims_rows_the_session_does_not_fit() -> Result<()> {
    // -- Setup & Fixtures: 2_000 consumed tokens against a 1_000-token row.
    // The fitting row is first so the default selection never lands on the
    // dimmed row.
    let mut driver = RenderLoopDriver::new(120, 30);
    let state = &mut driver.coordinator_mut().state;
    state.status.tokens.hydrate_latest_total_tokens(2_000);
    state.status.identity.active_model_identity = "ollama/large".to_string();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            model_option_with_window("ollama", "large", "Ollama", Some(5_000)),
            model_option_with_window("ollama", "small", "Ollama", Some(1_000)),
            model_option_with_window("ollama", "unknown", "Ollama", None),
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = driver.state().scroll.transcript_area;
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let palette = crate::rendering::theme::CatppuccinPalette::mocha();

    let small = identity_cell(&driver, &lines, popup, "ollama/small")?;
    assert_eq!(
        small.fg,
        Some(palette.overlay1),
        "an over-context row must render in overlay1\n{popup_text}"
    );
    assert!(
        small.add_modifier.contains(ratatui::style::Modifier::DIM),
        "an over-context row must be dimmed\n{popup_text}"
    );

    for (label, identity) in [
        ("a fitting row", "ollama/large"),
        ("a row with an unknown window", "ollama/unknown"),
    ] {
        let cell = identity_cell(&driver, &lines, popup, identity)?;
        assert!(
            !cell.add_modifier.contains(ratatui::style::Modifier::DIM),
            "{label} must not be dimmed\n{popup_text}"
        );
    }
    Ok(())
}

/// The active model is not a switch target, so it never dims — even when the
/// session's consumed tokens exceed its window.
#[tokio::test]
async fn model_picker_never_dims_the_active_model() -> Result<()> {
    // -- Setup & Fixtures: 2_000 consumed tokens, and the 1_000-token row is
    // the active model.
    let mut driver = RenderLoopDriver::new(120, 30);
    let state = &mut driver.coordinator_mut().state;
    state.status.tokens.hydrate_latest_total_tokens(2_000);
    state.status.identity.active_model_identity = "ollama/small".to_string();
    state.set_picker_options(
        ActivePicker::Model,
        vec![
            model_option_with_window("ollama", "large", "Ollama", Some(5_000)),
            model_option_with_window("ollama", "small", "Ollama", Some(1_000)),
        ],
    );
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let area = driver.state().scroll.transcript_area;
    let popup = super::render::frame::modal_rect_for_panel(
        area,
        super::render::frame::ModalPanelKind::Models,
    );
    let popup_text = driver.buffer_text_in_rect(popup);
    let lines: Vec<&str> = popup_text.lines().collect();
    let active = identity_cell(&driver, &lines, popup, "ollama/small")?;
    assert!(
        !active.add_modifier.contains(ratatui::style::Modifier::DIM),
        "the active model row must never be dimmed\n{popup_text}"
    );
    Ok(())
}
