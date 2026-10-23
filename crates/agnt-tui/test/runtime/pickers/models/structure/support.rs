//! Shared fixtures for the model picker structure tests.

use super::*;

/// The style of the cell holding `identity`'s first character.
pub(super) fn identity_cell(
    driver: &RenderLoopDriver,
    lines: &[&str],
    popup: ratatui::layout::Rect,
    identity: &str,
) -> Result<ratatui::style::Style> {
    let row = lines
        .iter()
        .position(|line| line.contains(identity))
        .ok_or(format!("should find the {identity} row"))?;
    let identity_at = lines[row]
        .find(identity)
        .ok_or(format!("{identity} row should contain the identity"))?;
    let identity_col = lines[row][..identity_at].chars().count();
    let x = popup.x + u16::try_from(identity_col)?;
    let y = popup.y + u16::try_from(row)?;
    Ok(driver.cell_style_at(x, y))
}

pub(super) fn model_option(
    provider: &str,
    model: &str,
    provider_display_name: &str,
) -> agnt_core::protocol::picker::ModelPickerOption {
    agnt_core::protocol::picker::ModelPickerOption {
        provider: provider.to_string(),
        model: model.to_string(),
        identity: format!("{provider}/{model}"),
        display: format!("{provider}/{model}"),
        active: false,
        context_window: None,
        max_output: None,
        configured: true,
        provider_display_name: provider_display_name.to_string(),
        cost: None,
    }
}

/// The same option with a known context window.
pub(super) fn model_option_with_window(
    provider: &str,
    model: &str,
    provider_display_name: &str,
    context_window: Option<u32>,
) -> agnt_core::protocol::picker::ModelPickerOption {
    agnt_core::protocol::picker::ModelPickerOption {
        context_window,
        ..model_option(provider, model, provider_display_name)
    }
}
