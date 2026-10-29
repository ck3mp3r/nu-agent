use super::*;

use crate::state::{InfoPanel, McpServerState, McpServerUsabilityState};

/// Every modal popup sizes, clamps, and centers against the transcript pane, so
/// at a short terminal no popup reaches past the pane into the input and status
/// box below it.
#[tokio::test]
async fn test_modal_popups_stay_inside_transcript_pane_at_eight_rows() -> Result<()> {
    // -- Setup & Fixtures: one surface per iteration, the five pickers plus the
    // Mcps info panel.
    let surfaces = ["palette", "models", "agent", "sessions", "themes", "mcps"];

    for label in surfaces {
        // -- Setup & Fixtures: a fresh 8-row driver per surface.
        let mut driver = RenderLoopDriver::new(60, 8);
        open_surface(&mut driver.coordinator_mut().state, label);

        // -- Exec & Check: the rendered popup border must lie inside the pane.
        driver.advance_with_frame(&[]).await?;
        let (x, y, width, height) =
            rendered_popup_rect(&driver).ok_or("should render the popup border")?;
        let pane = driver.state().scroll.transcript_area;
        assert!(
            x >= pane.x
                && y >= pane.y
                && x + width <= pane.x + pane.width
                && y + height <= pane.y + pane.height,
            "{label}: popup ({x}, {y}, {width}, {height}) must stay inside the transcript pane \
             ({}, {}, {}, {}); buffer:\n{}",
            pane.x,
            pane.y,
            pane.width,
            pane.height,
            driver.buffer_text()
        );
    }
    Ok(())
}

// region:    --- Test Support

/// Opens one modal surface on the state: a picker for the picker labels, the
/// Mcps info panel with one failed server otherwise.
fn open_surface(state: &mut AppState, label: &str) {
    match label {
        "palette" => {
            state.picker.open(ActivePicker::CommandPalette);
        }
        "models" => {
            state.picker.open(ActivePicker::Model);
        }
        "agent" => {
            state.picker.open(ActivePicker::Agent);
        }
        "sessions" => {
            state.picker.open(ActivePicker::Session);
        }
        "themes" => {
            state.picker.open(ActivePicker::Theme);
        }
        _ => {
            state.status.mcp.set_mcp_servers(vec![McpServerState {
                name: "k8s".to_string(),
                state: McpServerUsabilityState::Failed,
            }]);
            state.status.mcp.set_mcp_server_state_by_name_with_reason(
                "k8s",
                McpServerUsabilityState::Failed,
                Some("connection refused".to_string()),
            );
            state.open_info_panel(InfoPanel::Mcps);
        }
    }
}

// endregion: --- Test Support
