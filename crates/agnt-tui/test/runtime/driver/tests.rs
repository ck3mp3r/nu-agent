//! Tests for the real render loop, driven by
//! [`RenderLoopDriver`](super::RenderLoopDriver).
//!
//! Split from the driver itself to keep both files under the test-file size
//! limit; the shared imports arrive through `use super::*`.

use super::*;

use agnt_runtime::bus::{ControlCommand, SharedUiAction};

#[tokio::test]
async fn render_loop_driver_test_backend_buffer_observes_sentinel_after_event() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec & Check: before any scripted event the buffer must not contain
    // the sentinel.
    driver.advance(&[]).await?;
    let before = driver.buffer_text();
    assert!(
        !before.contains("SENTINEL"),
        "sentinel must be absent before the event, got:\n{before}"
    );

    // -- Exec: type + submit the sentinel through the real terminal arm.
    let mut script: Vec<DriveEvent> = "SENTINEL"
        .chars()
        .map(|c| {
            DriveEvent::Key(TerminalEvent::Key(
                crate::interaction::input::TerminalKey::Char(c),
            ))
        })
        .collect();
    script.push(DriveEvent::Key(TerminalEvent::Key(
        crate::interaction::input::TerminalKey::Enter,
    )));
    driver.advance_with_frame(&script).await?;

    // -- Check: the rendered frame produced by the real loop must contain the
    // sentinel user prompt.
    let after = driver.buffer_text();
    assert!(
        after.contains("SENTINEL"),
        "TestBackend buffer must contain the sentinel after the loop renders it, got:\n{after}"
    );
    Ok(())
}

#[tokio::test]
async fn render_loop_driver_routes_tool_record_through_bus_stream_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec: publish a stream record like production stages do.
    driver
        .advance(&[DriveEvent::Record(Box::new(SessionRecord::Tool(
            agnt_runtime::protocol::events::ToolRecord::Started {
                call: agnt_runtime::protocol::events::ToolCall::new(
                    agnt_render::transcript::ir::ToolName("sentinel_tool".to_string()),
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    "{}",
                ),
            },
        )))])
        .await?;

    // -- Check: the loop's ui_event arm reduced the event (the transcript
    // records the tool entry). The entry carries the call line, not the tool
    // name, so assert a Tool entry materialized with in-progress status.
    assert!(
        driver
            .state()
            .transcript
            .entries()
            .iter()
            .any(|entry| matches!(entry, TranscriptEntry::Tool(tool) if tool.status == ItemStatus::InProgress)),
        "tool event must reach the coordinator through the real bus arm"
    );
    Ok(())
}

/// Every popup renders its navigation keys in the title, with " · " between
/// hint items: scroll panels advertise ↑/↓ · PgUp/PgDn · Esc, pickers advertise
/// ↑/↓ · Enter · Esc.
#[tokio::test]
async fn every_popup_title_shows_its_navigation_keys() -> Result<()> {
    // -- Setup & Fixtures: one popup per case, opened through the real state.
    let cases: [(&str, &str); 9] = [
        ("status", "Status (↑/↓ · PgUp/PgDn · Esc)"),
        ("skills", "Skills (↑/↓ · PgUp/PgDn · Esc)"),
        ("help", "Help (↑/↓ · PgUp/PgDn · Esc"),
        ("palette", "Command Palette (↑/↓ · Enter · Esc"),
        ("models", "Models (↑/↓ · Enter · Esc)"),
        ("agent", "Agent (↑/↓ · Enter · Esc)"),
        ("sessions", "Sessions (↑/↓ · Enter · Esc"),
        ("theme", "Theme (↑/↓ · Enter · Esc)"),
        ("inline_slash", "Commands (↑/↓ · Enter · Esc)"),
    ];

    for (label, expected) in cases {
        // -- Setup & Fixtures: a fresh driver per popup.
        let mut driver = RenderLoopDriver::new(160, 40);
        {
            let state = &mut driver.coordinator_mut().state;
            match label {
                "status" => state.open_info_panel(crate::state::InfoPanel::Status),
                "skills" => state.open_info_panel(crate::state::InfoPanel::Skills),
                "help" => state.open_info_panel(crate::state::InfoPanel::Help),
                "palette" => {
                    state
                        .picker
                        .open(crate::state::ActivePicker::CommandPalette);
                }
                "models" => {
                    state.picker.open(crate::state::ActivePicker::Model);
                }
                "agent" => {
                    state.picker.open(crate::state::ActivePicker::Agent);
                }
                "sessions" => {
                    state.picker.open(crate::state::ActivePicker::Session);
                }
                "inline_slash" => state.check_inline_slash("/"),
                _ => {
                    state.picker.open(crate::state::ActivePicker::Theme);
                }
            }
        }

        // -- Exec: settle past the render throttle so a frame lands.
        driver.advance_with_frame(&[]).await?;

        // -- Check: the rendered title carries the keys, never a bare " | ".
        let buffer = driver.buffer_text();
        assert!(
            buffer.contains(expected),
            "{label}: expected title fragment {expected:?} in:\n{buffer}"
        );
    }
    Ok(())
}

/// A narration chunk published on the bus reaches the coordinator through the
/// loop's narration arm, the same way production stages publish it.
#[tokio::test]
async fn render_loop_driver_routes_narration_through_bus_narration_arm() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);

    // -- Exec
    driver
        .advance(&[DriveEvent::Narration(Box::new(
            NarrationEvent::InferenceChunk {
                delta: "SENTINEL".to_string(),
                aggregated: "SENTINEL".to_string(),
            },
        ))])
        .await?;

    // -- Check: the chunk landed as the streamed assistant block.
    assert!(
        driver.state().transcript.entries().iter().any(|entry| {
            matches!(
                entry,
                TranscriptEntry::Message(message) if message.markdown.contains("SENTINEL")
            )
        }),
        "narration must reach the coordinator through the real bus arm"
    );
    Ok(())
}

/// WHEN a control command folds state, THE render loop SHALL paint the change on
/// its next tick: the fold marks the frame dirty, and no key press follows it.
#[tokio::test]
async fn control_intake_fold_paints_the_frame_without_another_event() -> Result<()> {
    // -- Setup & Fixtures: an idle loop, settled so a base frame lands.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.advance_with_frame(&[]).await?;

    // -- Check: the base frame carries no status panel.
    let before = driver.buffer_text();
    assert!(
        !before.contains("Status (↑/↓"),
        "the base frame must not show the status panel, got:\n{before}"
    );

    // -- Exec: the status handoff arrives on the control channel alone.
    driver
        .advance_with_frame(&[DriveEvent::Control(Box::new(
            ControlCommand::SharedUiAction {
                action: SharedUiAction::Status,
            },
        ))])
        .await?;

    // -- Check: the tick painted the panel the fold opened.
    let after = driver.buffer_text();
    assert!(
        after.contains("Status (↑/↓"),
        "a control fold must repaint without another event, got:\n{after}"
    );
    Ok(())
}
