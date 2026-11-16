//! Tests for the real render loop, driven by
//! [`RenderLoopDriver`](super::RenderLoopDriver).
//!
//! Split from the driver itself to keep both files under the test-file size
//! limit; the shared imports arrive through `use super::*`.

use super::*;

use agnt_render::transcript::ir::MessageRole;
use agnt_runtime::bus::{ControlCommand, SharedUiAction};
use agnt_runtime::protocol::events::{InferenceId, InferenceRecord};

// region:    --- Modules

// The lane fold order lives in its own file: four 16-round pins would push this
// file past the test-file size limit.
#[path = "tests/ordering.rs"]
mod ordering;

// endregion: --- Modules

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
                inference: crate::test_support::inference(),
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

/// WHEN a key event and a text-lane record are both ready at one wake, THE
/// render loop SHALL fold the key before the record fold of that pass: the
/// transcript keeps the fold order, so the submitted row lands ahead of the
/// block the record streams into.
#[tokio::test]
async fn test_render_loop_prioritizes_input_over_text_burst() -> Result<()> {
    // -- Setup & Fixtures
    const ROUNDS: usize = 16;
    const PROMPT: &str = "priority";

    // -- Exec & Check: one round pins nothing — which arm the loop wakes on is
    // the loop's own business when the select is unbiased. Each round publishes
    // its whole script before yielding once, so the key and the text lane are
    // both ready at the wake that follows.
    for round in 0..ROUNDS {
        // -- Setup & Fixtures: a fresh transcript per round, so the two ranks
        // below are the round's own fold order.
        let mut driver = RenderLoopDriver::new(160, 40);
        let streamed = format!("burst-{round:02}");
        let mut script: Vec<DriveEvent> = PROMPT
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
        script.push(DriveEvent::TextRecord(Box::new(streamed_chunk(
            crate::test_support::inference(),
            &streamed,
        ))));

        // -- Exec
        driver.advance_burst(&script).await?;

        // -- Check: the key's submit appends the user row, the text record
        // appends the assistant block, and the transcript keeps them in the
        // order the loop folded them.
        let entries = driver.state().transcript.entries();
        let key_rank = entries
            .iter()
            .position(|entry| {
                matches!(entry, TranscriptEntry::Message(message)
                    if message.role == MessageRole::User && message.markdown.contains(PROMPT))
            })
            .ok_or("the key fold must append the submitted user row")?;
        let record_rank = entries
            .iter()
            .position(|entry| {
                matches!(entry, TranscriptEntry::Message(message)
                    if message.markdown.contains(&streamed))
            })
            .ok_or("the text record must append the streamed block")?;
        assert!(
            key_rank < record_rank,
            "round {round}: the key fold must precede the record fold, \
             got the key at {key_rank} and the record at {record_rank}"
        );
    }
    Ok(())
}

/// WHEN more terminal events than one pass may fold are ready at one wake, THE
/// render loop SHALL fold every one of them across passes and still reach the
/// bus lanes: the drain bound defers the tail, it never drops it.
#[tokio::test]
async fn test_render_loop_folds_an_input_burst_past_the_drain_bound() -> Result<()> {
    // -- Setup & Fixtures: more typed keys than one pass folds, plus a text
    // record that can only fold once the input queue has given way. The typed
    // unit never repeats a letter, so no insert-mode chord fires.
    const UNIT: &str = "abcdefghik";
    let typed = UNIT.repeat(crate::interactive::INPUT_BURST_DRAIN_MAX / UNIT.len() + 1);
    let mut driver = RenderLoopDriver::new(160, 40);
    let mut script: Vec<DriveEvent> = typed
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
    script.push(DriveEvent::TextRecord(Box::new(streamed_chunk(
        crate::test_support::inference(),
        "past-bound",
    ))));

    // -- Exec
    driver.advance_burst(&script).await?;

    // -- Check: every queued key folded, in order, before the submit.
    let entries = driver.state().transcript.entries();
    let submitted = entries.iter().find_map(|entry| match entry {
        TranscriptEntry::Message(message) if message.role == MessageRole::User => {
            Some(message.markdown.clone())
        }
        _ => None,
    });
    assert_eq!(
        submitted.as_deref(),
        Some(typed.as_str()),
        "the bound defers the queued keys to the next pass, it never drops them"
    );

    // -- Check: the bus lane still folded, so the input drain never starves it.
    assert!(
        entries.iter().any(|entry| matches!(
            entry,
            TranscriptEntry::Message(message) if message.markdown.contains("past-bound")
        )),
        "a text record pending behind an input burst must still fold"
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

// -- Test Support

/// The text-lane record a streaming producer emits: one delta carrying the whole
/// text so far, under the call it belongs to.
fn streamed_chunk(inference: InferenceId, aggregated: &str) -> SessionRecord {
    SessionRecord::Inference(InferenceRecord::Chunk {
        inference,
        delta: aggregated.to_string(),
        aggregated: aggregated.to_string(),
    })
}
