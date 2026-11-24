//! The pickers' fallback path: an option whose payload the drawing surface does
//! not enumerate renders a defined state — a blank row, a label taken from the
//! option's own data, or the option's display text — and never aborts.

use super::*;

use crate::state::{PickerOption, PickerPayload};

/// WHEN the models picker holds an option whose payload is not a model, THE
/// models drawer SHALL paint a blank row under the option's own section key and
/// SHALL NOT abort.
#[tokio::test]
async fn test_model_picker_draws_fallback_for_unhandled_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Model, vec![unhandled_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Model);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Models,
    );
    let text = driver.buffer_text_in_rect(popup);
    assert!(text.contains("MODEL"), "the table keeps its header\n{text}");
    assert!(
        text.contains(UNHANDLED_SECTION),
        "the fallback head labels its group from the option's own section key\n{text}"
    );
    assert!(
        !text.contains(UNHANDLED_DISPLAY),
        "the unhandled option paints no content\n{text}"
    );
    assert_blank_row_drawn(&text);
    Ok(())
}

/// WHEN the agent picker holds an option whose payload is not an agent, THE
/// agent drawer SHALL paint a blank row and SHALL NOT abort.
#[tokio::test]
async fn test_agent_picker_draws_fallback_for_unhandled_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Agent, vec![unhandled_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Agents,
    );
    let text = driver.buffer_text_in_rect(popup);
    assert!(
        text.contains("Description"),
        "the table keeps its header\n{text}"
    );
    assert!(
        !text.contains(UNHANDLED_DISPLAY),
        "the unhandled option paints no content\n{text}"
    );
    assert_blank_row_drawn(&text);
    Ok(())
}

/// WHEN the session picker holds an option whose payload is not a session, THE
/// session drawer SHALL paint a blank row under a blank day head and SHALL NOT
/// abort.
#[tokio::test]
async fn test_session_picker_draws_fallback_for_unhandled_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .coordinator_mut()
        .state
        .set_picker_options(ActivePicker::Session, vec![unhandled_option()]);
    driver
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Session);

    // -- Exec
    driver.advance_with_frame(&[]).await?;

    // -- Check
    let popup = super::render::frame::modal_rect_for_panel(
        driver.state().scroll.transcript_area,
        super::render::frame::ModalPanelKind::Sessions,
    );
    let text = driver.buffer_text_in_rect(popup);
    assert!(text.contains("When"), "the table keeps its header\n{text}");
    assert!(
        !text.contains(UNHANDLED_SECTION),
        "an unhandled payload opens no day head\n{text}"
    );
    assert!(
        !text.contains(UNHANDLED_DISPLAY),
        "the unhandled option paints no content\n{text}"
    );
    assert_blank_row_drawn(&text);
    Ok(())
}

/// WHEN the inline slash list holds an option whose payload is not a slash
/// command, THE list SHALL draw the option's own display text under the marker
/// and SHALL NOT abort.
#[tokio::test]
async fn test_inline_slash_draws_fallback_for_unhandled_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::InlineSlash, vec![unhandled_option()]);
    state.picker.open(ActivePicker::InlineSlash);

    // -- Exec
    let lines = crate::runtime::inline_slash_lines_for_test(&state);

    // -- Check
    assert_eq!(
        lines,
        vec![format!("❯ {UNHANDLED_DISPLAY}")],
        "the fallback keeps the row and the marker and draws the display text"
    );
    Ok(())
}

/// WHEN the command palette holds an option whose payload is not a command, THE
/// palette SHALL draw the option's own display text as its label with no
/// summary and SHALL NOT abort.
#[test]
fn test_command_palette_draws_fallback_for_unhandled_payload() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.set_picker_options(ActivePicker::CommandPalette, vec![unhandled_option()]);
    state.picker.open(ActivePicker::CommandPalette);

    // -- Exec
    let model = crate::runtime::command_palette_table_model_for_test(&state, 80, 10);

    // -- Check
    assert_eq!(model.rows.len(), 1, "the fallback keeps the row");
    assert!(
        UNHANDLED_DISPLAY.starts_with(model.rows[0][0].trim_end_matches('…')),
        "the label derives from the option's own display text; rows: {:?}",
        model.rows
    );
    assert!(
        model.rows[0][0].ends_with('…'),
        "the label fills the Action column; rows: {:?}",
        model.rows
    );
    assert_eq!(model.rows[0][1], "", "the fallback carries no summary");
    Ok(())
}

// region:    --- Test Support

const UNHANDLED_DISPLAY: &str = "unhandled-payload-display";
const UNHANDLED_SECTION: &str = "unhandled-payload-section";

/// An option whose payload no drawing surface enumerates: the theme variant is
/// built by the theme picker alone, so every other surface meets it as a
/// payload it does not know.
fn unhandled_option() -> PickerOption {
    PickerOption {
        id: "unhandled-payload-row".to_string(),
        display: UNHANDLED_DISPLAY.to_string(),
        search_text: UNHANDLED_DISPLAY.to_string(),
        section: Some(UNHANDLED_SECTION.to_string()),
        sort_key: Vec::new(),
        payload: PickerPayload::Theme,
    }
}

/// The picker table paints its highlight symbol on the selected row, so a
/// selected row that carries no text still proves the fallback row was drawn
/// rather than dropped.
fn assert_blank_row_drawn(text: &str) {
    assert!(
        text.contains('❯'),
        "the fallback row must be drawn and selected, not dropped\n{text}"
    );
}

// endregion: --- Test Support
