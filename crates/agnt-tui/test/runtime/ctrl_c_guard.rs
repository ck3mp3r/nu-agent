use super::*;

use crate::interaction::input::{TerminalMouse, TerminalResize};
use crate::rendering::layout::INPUT_MIN_HEIGHT;
use crate::state::{CTRL_C_QUIT_CONFIRM_STATUS, InfoPanel, PermissionPrompt};

// region:    --- Test Support

fn open_permission_prompt(state: &mut AppState) {
    state.permission.open_prompt(PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu.command:*".to_string(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
    });
}

fn ctrl_c() -> DriveEvent {
    key(TerminalKey::CtrlC)
}

fn draft(driver: &RenderLoopDriver) -> String {
    driver.coordinator().textarea.lines().join("\n")
}

fn set_draft(driver: &mut RenderLoopDriver, text: &str) {
    driver.coordinator_mut().textarea.insert_str(text);
}

fn status_message(driver: &RenderLoopDriver) -> &str {
    driver.state().status.message.status_line()
}

fn armed(driver: &RenderLoopDriver) -> bool {
    driver.state().quit_confirm_since.is_some()
}

// endregion: --- Test Support

// region:    --- Tests

/// A permission prompt keeps ownership of its own keys: Ctrl-C arms the quit
/// confirmation, leaves the prompt open, and submits no decision.
#[tokio::test]
async fn test_ctrl_c_guard_permission_prompt_arms_without_decision() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    open_permission_prompt(&mut driver.coordinator_mut().state);
    set_draft(&mut driver, "draft");

    // -- Exec
    driver.advance_with_frame(&[ctrl_c()]).await?;

    // -- Check
    assert!(driver.state().permission.has_prompt());
    assert!(
        driver
            .coordinator_mut()
            .state
            .permission
            .take_next_submission()
            .is_none(),
        "Ctrl-C must not submit a permission decision"
    );
    assert!(armed(&driver));
    assert_eq!(status_message(&driver), CTRL_C_QUIT_CONFIRM_STATUS);
    assert!(!driver.coordinator().quit_requested());
    assert_eq!(draft(&driver), "draft");
    assert!(
        driver.buffer_text().contains(CTRL_C_QUIT_CONFIRM_STATUS),
        "the confirmation message must be rendered, got:\n{}",
        driver.buffer_text()
    );
    Ok(())
}

/// Every popup kind closes on Ctrl-C without arming and without touching the
/// draft.
#[tokio::test]
async fn test_ctrl_c_guard_popup_closes_without_arming() -> Result<()> {
    // -- Setup & Fixtures: a picker.
    let mut picker = RenderLoopDriver::new(120, 30);
    picker
        .coordinator_mut()
        .state
        .picker
        .open(ActivePicker::Agent);
    set_draft(&mut picker, "draft");

    // -- Exec
    picker.advance(&[ctrl_c()]).await?;

    // -- Check
    assert!(picker.state().picker.active().is_none());
    assert!(!armed(&picker));
    assert!(status_message(&picker).is_empty());
    assert_eq!(draft(&picker), "draft");

    // -- Setup & Fixtures: an info panel.
    let mut panel = RenderLoopDriver::new(120, 30);
    panel
        .coordinator_mut()
        .state
        .open_info_panel(InfoPanel::Help);
    set_draft(&mut panel, "draft");

    // -- Exec
    panel.advance(&[ctrl_c()]).await?;

    // -- Check
    assert!(panel.state().info_panel.is_none());
    assert!(!armed(&panel));
    assert!(status_message(&panel).is_empty());
    assert_eq!(draft(&panel), "draft");

    // -- Setup & Fixtures: the inline slash popup.
    let mut slash = RenderLoopDriver::new(120, 30);
    set_draft(&mut slash, "/");
    slash.coordinator_mut().state.check_inline_slash("/");
    assert_eq!(
        slash.state().picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

    // -- Exec
    slash.advance(&[ctrl_c()]).await?;

    // -- Check
    assert!(slash.state().picker.active().is_none());
    assert!(!armed(&slash));
    assert!(status_message(&slash).is_empty());
    assert_eq!(draft(&slash), "/");
    Ok(())
}

/// A non-empty draft is cleared on Ctrl-C without arming, in any phase.
#[tokio::test]
async fn test_ctrl_c_guard_clears_draft_without_arming() -> Result<()> {
    // -- Setup & Fixtures: idle with a draft.
    let mut idle = RenderLoopDriver::new(120, 30);
    idle.advance(&[key(TerminalKey::Char('d')), key(TerminalKey::Char('r'))])
        .await?;
    assert_eq!(draft(&idle), "dr");

    // -- Exec
    idle.advance(&[ctrl_c()]).await?;

    // -- Check
    assert_eq!(draft(&idle), "");
    assert!(!armed(&idle));
    assert!(status_message(&idle).is_empty());
    assert_eq!(idle.state().phase, UiPhase::Idle);
    assert!(!idle.coordinator().quit_requested());

    // -- Setup & Fixtures: busy with a draft.
    let mut busy = RenderLoopDriver::new(120, 30);
    busy.advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(busy.state().phase, UiPhase::Busy);
    busy.advance(&[key(TerminalKey::Char('d'))]).await?;
    assert_eq!(draft(&busy), "d");

    // -- Exec
    busy.advance(&[ctrl_c()]).await?;

    // -- Check: the draft clears, the turn keeps running.
    assert_eq!(draft(&busy), "");
    assert!(!armed(&busy));
    assert!(status_message(&busy).is_empty());
    assert_eq!(busy.state().phase, UiPhase::Busy);
    assert!(!busy.coordinator().quit_requested());
    assert!(!busy.coordinator().take_cancel_requested());
    Ok(())
}

/// Clearing a wrapped draft shrinks the input box back to its minimum height.
#[tokio::test]
async fn test_ctrl_c_guard_clear_draft_resets_input_height() -> Result<()> {
    // -- Setup & Fixtures: a draft long enough to wrap.
    let mut driver = RenderLoopDriver::new(120, 30);
    driver
        .advance(&[DriveEvent::Key(TerminalEvent::Paste("x".repeat(200)))])
        .await?;
    assert!(
        driver.coordinator().input_height > INPUT_MIN_HEIGHT,
        "the wrapped draft must grow the input box"
    );

    // -- Exec
    driver.advance(&[ctrl_c()]).await?;

    // -- Check
    assert_eq!(draft(&driver), "");
    assert_eq!(driver.coordinator().input_height, INPUT_MIN_HEIGHT);
    Ok(())
}

/// An empty draft arms the confirmation; the second press quits, cancelling a
/// running turn.
#[tokio::test]
async fn test_ctrl_c_guard_arms_then_confirms_quit() -> Result<()> {
    // -- Setup & Fixtures: idle with an empty draft.
    let mut idle = RenderLoopDriver::new(120, 30);

    // -- Exec
    idle.advance(&[ctrl_c()]).await?;

    // -- Check: armed, not quit.
    assert!(armed(&idle));
    assert_eq!(status_message(&idle), CTRL_C_QUIT_CONFIRM_STATUS);
    assert!(!idle.coordinator().quit_requested());

    // -- Exec
    idle.advance(&[ctrl_c()]).await?;

    // -- Check: quit.
    assert!(idle.coordinator().quit_requested());
    assert!(idle.state().quit_requested);

    // -- Setup & Fixtures: a busy turn with an empty draft.
    let mut busy = RenderLoopDriver::new(120, 30);
    busy.advance(&[key(TerminalKey::Char('x')), key(TerminalKey::Enter)])
        .await?;
    assert_eq!(busy.state().phase, UiPhase::Busy);

    // -- Exec
    busy.advance(&[ctrl_c()]).await?;

    // -- Check: armed, the turn keeps running.
    assert!(armed(&busy));
    assert!(!busy.coordinator().quit_requested());
    assert_eq!(busy.state().phase, UiPhase::Busy);

    // -- Exec
    busy.advance(&[ctrl_c()]).await?;

    // -- Check: quit and cancel the running turn.
    assert!(busy.coordinator().quit_requested());
    assert!(
        busy.coordinator().take_cancel_requested(),
        "the confirming press must request cancel while busy"
    );
    Ok(())
}

/// Any other key disarms the confirmation and is then routed normally.
#[tokio::test]
async fn test_ctrl_c_guard_other_key_disarms() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.advance(&[ctrl_c()]).await?;
    assert!(armed(&driver));

    // -- Exec
    driver.advance(&[key(TerminalKey::Char('a'))]).await?;

    // -- Check: disarmed, message cleared, the key still reached the textarea.
    assert!(!armed(&driver));
    assert!(status_message(&driver).is_empty());
    assert_eq!(draft(&driver), "a");
    assert!(!driver.coordinator().quit_requested());
    Ok(())
}

/// A paste disarms the confirmation and is then routed normally.
#[tokio::test]
async fn test_ctrl_c_guard_paste_disarms() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.advance(&[ctrl_c()]).await?;
    assert!(armed(&driver));

    // -- Exec
    driver
        .advance(&[DriveEvent::Key(TerminalEvent::Paste("hi".to_string()))])
        .await?;

    // -- Check
    assert!(!armed(&driver));
    assert!(status_message(&driver).is_empty());
    assert_eq!(draft(&driver), "hi");
    Ok(())
}

/// An elapsed window disarms on the next event and leaves the message to its
/// own TTL; a resize leaves the armed state untouched.
#[tokio::test]
async fn test_ctrl_c_guard_expiry_and_resize() -> Result<()> {
    // -- Setup & Fixtures: a zero-length window.
    let mut expired = RenderLoopDriver::new(120, 30);
    expired.coordinator_mut().state.quit_confirm_window = Duration::from_millis(0);

    // -- Exec
    expired.advance(&[ctrl_c()]).await?;

    // -- Check: armed but already outside the window.
    assert!(armed(&expired));
    assert!(!expired.state().quit_confirmation_armed());

    // -- Exec: a second press re-arms instead of quitting.
    expired.advance(&[ctrl_c()]).await?;

    // -- Check
    assert!(!expired.coordinator().quit_requested());
    assert!(armed(&expired));

    // -- Exec: another key disarms without clearing the message.
    expired.advance(&[key(TerminalKey::Char('a'))]).await?;

    // -- Check
    assert!(!armed(&expired));
    assert_eq!(status_message(&expired), CTRL_C_QUIT_CONFIRM_STATUS);

    // -- Setup & Fixtures: an armed confirmation and a resize.
    let mut resized = RenderLoopDriver::new(120, 30);
    resized.advance(&[ctrl_c()]).await?;
    assert!(armed(&resized));

    // -- Exec
    resized
        .advance(&[DriveEvent::Key(TerminalEvent::Resize(TerminalResize {
            columns: 100,
            rows: 40,
        }))])
        .await?;

    // -- Check: the armed state survives a resize.
    assert!(armed(&resized));
    assert_eq!(status_message(&resized), CTRL_C_QUIT_CONFIRM_STATUS);
    Ok(())
}

/// A mouse wheel event disarms the confirmation and is routed to dispatch.
#[tokio::test]
async fn test_ctrl_c_guard_mouse_disarms() -> Result<()> {
    // -- Setup & Fixtures
    let mut driver = RenderLoopDriver::new(120, 30);
    driver.advance(&[ctrl_c()]).await?;
    assert!(armed(&driver));

    // -- Exec
    driver
        .advance(&[DriveEvent::Key(TerminalEvent::Mouse(TerminalMouse {
            up: true,
            column: 10,
            row: 5,
        }))])
        .await?;

    // -- Check
    assert!(!armed(&driver));
    assert!(!driver.coordinator().quit_requested());
    Ok(())
}

/// A mouse event that disarms an armed confirmation marks the frame dirty.
#[test]
fn test_ctrl_c_guard_mouse_disarm_marks_render() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(120, 30, None);
    coord.state.arm_quit_confirmation();
    coord.set_render_needed(false);

    // -- Exec
    let consumed = coord.handle_ctrl_c_guard(&TerminalEvent::Mouse(TerminalMouse {
        up: true,
        column: 10,
        row: 5,
    }));

    // -- Check: not consumed, the confirm message cleared, the frame dirty.
    assert!(!consumed);
    assert!(coord.state.status.message.status_line().is_empty());
    assert!(coord.render_needed());
    Ok(())
}

/// A mouse event that disarms nothing leaves the frame clean.
#[test]
fn test_ctrl_c_guard_mouse_without_arm_leaves_render_clean() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(120, 30, None);
    coord.set_render_needed(false);

    // -- Exec
    let consumed = coord.handle_ctrl_c_guard(&TerminalEvent::Mouse(TerminalMouse {
        up: true,
        column: 10,
        row: 5,
    }));

    // -- Check
    assert!(!consumed);
    assert!(!coord.render_needed());
    Ok(())
}

/// Only the Ctrl-C key event is consumed by the guard; mouse, paste, other
/// keys, and resize are routed onward.
#[test]
fn test_ctrl_c_guard_consumes_only_ctrl_c() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(120, 30, None);

    // -- Exec & Check
    assert!(
        coord.handle_ctrl_c_guard(&TerminalEvent::Key(TerminalKey::CtrlC)),
        "Ctrl-C must be consumed by the guard"
    );
    assert!(
        !coord.handle_ctrl_c_guard(&TerminalEvent::Mouse(TerminalMouse {
            up: true,
            column: 10,
            row: 5,
        })),
        "a mouse event must be routed onward"
    );
    assert!(
        !coord.handle_ctrl_c_guard(&TerminalEvent::Paste("hi".to_string())),
        "a paste must be routed onward"
    );
    assert!(
        !coord.handle_ctrl_c_guard(&TerminalEvent::Key(TerminalKey::Char('a'))),
        "a non-Ctrl-C key must be routed onward"
    );
    assert!(
        !coord.handle_ctrl_c_guard(&TerminalEvent::Resize(TerminalResize {
            columns: 100,
            rows: 40,
        })),
        "a resize must be routed onward"
    );
    Ok(())
}

// endregion: --- Tests
