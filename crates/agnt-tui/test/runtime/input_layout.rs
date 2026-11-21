//! Input-layout recompute gating: which terminal events pay for a draft
//! re-wrap.
//!
//! Only an event that can write the draft changes the input's wrapped height, so
//! only such an event recomputes it. A wheel notch scrolls the transcript and
//! mutates no input, so a long draft must not cost O(draft) per scroll step.

use super::*;

/// A wheel notch at the transcript's middle.
const WHEEL_DOWN: TerminalEvent = TerminalEvent::Mouse(TerminalMouse {
    up: false,
    column: 40,
    row: 12,
});

/// WHEN a wheel notch folds, THEN THE input layout recompute SHALL NOT run: the
/// notch scrolls the transcript and mutates no draft.
#[test]
fn test_runtime_input_layout_wheel_fold_skips_the_recompute() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);
    coordinator.set_render_needed(false);

    // -- Exec
    coordinator.handle_terminal_event(WHEEL_DOWN);

    // -- Check
    assert_eq!(
        coordinator.layout_recomputes(),
        0,
        "a wheel notch cannot mutate the draft, so it must not re-wrap it"
    );
    Ok(())
}

/// WHEN a character key folds into the draft, THEN THE input layout recompute
/// SHALL run exactly once for that event.
#[test]
fn test_runtime_input_layout_key_fold_recomputes_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Char('x')));

    // -- Check
    assert_eq!(coordinator.textarea.lines().join("\n"), "x");
    assert_eq!(
        coordinator.layout_recomputes(),
        1,
        "a key that writes the draft must re-wrap it once"
    );
    Ok(())
}

/// WHEN a paste folds into the draft, THEN THE input layout recompute SHALL run
/// exactly once for that event.
#[test]
fn test_runtime_input_layout_paste_fold_recomputes_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Paste("pasted draft".to_string()));

    // -- Check
    assert_eq!(coordinator.textarea.lines().join("\n"), "pasted draft");
    assert_eq!(
        coordinator.layout_recomputes(),
        1,
        "a paste that writes the draft must re-wrap it once"
    );
    Ok(())
}

/// WHEN a resize folds, THEN THE input layout recompute SHALL run exactly once
/// for that event: the new content width is what the draft must be re-wrapped
/// at, and the recompute happens once, not twice.
#[test]
fn test_runtime_input_layout_resize_fold_recomputes_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, None);

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Resize(TerminalResize {
        columns: 100,
        rows: 30,
    }));

    // -- Check
    assert_eq!(
        coordinator.layout_recomputes(),
        1,
        "a resize changes the content width and must re-wrap exactly once"
    );
    Ok(())
}
