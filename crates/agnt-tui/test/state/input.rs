//! Unit tests for [`InputState`] mode rewriting, chords, and history.

use super::*;
use crate::interaction::reducer::UserAction;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Idle Normal-mode keymap: vim keys map to scroll/focus/mode actions, and
/// unmapped characters are consumed as no-ops. Idle rewriting never reports a
/// change (the reducer owns mode transitions).
#[test]
fn test_input_state_idle_normal_keymap_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Normal);

    // -- Exec & Check: mapped keys.
    let cases: [(char, UserAction); 8] = [
        ('j', UserAction::ScrollLineDown),
        ('k', UserAction::ScrollLineUp),
        ('h', UserAction::FocusPaneLeft),
        ('l', UserAction::FocusPaneRight),
        ('i', UserAction::EnterInsertMode),
        ('v', UserAction::EnterVisualMode),
        ('G', UserAction::ScrollToBottom),
        ('x', UserAction::Noop),
    ];
    for (ch, expected) in cases {
        let (action, changed) = input.handle_action(UserAction::InsertChar(ch), true);
        assert_eq!(action, expected, "key {ch}");
        assert!(!changed, "key {ch}");
    }

    // -- Exec & Check: completion keys map to pane focus.
    let (action, _) = input.handle_action(UserAction::CompleteForward, true);
    assert_eq!(action, UserAction::FocusPaneRight);
    let (action, _) = input.handle_action(UserAction::CompleteBackward, true);
    assert_eq!(action, UserAction::FocusPaneLeft);

    // -- Exec & Check: Esc passes through.
    let (action, _) = input.handle_action(UserAction::Esc, true);
    assert_eq!(action, UserAction::Esc);

    // -- Exec & Check: the gg chord arms then fires, then re-arms.
    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::Noop);
    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::ScrollToTop);
    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::Noop);

    // -- Exec & Check: Visual-mode 'y' maps to yank.
    let mut visual = InputState::default().with_mode(InputMode::Visual);
    let (action, _) = visual.handle_action(UserAction::InsertChar('y'), true);
    assert_eq!(action, UserAction::YankSelection);
    Ok(())
}

/// Idle Insert-mode chords and busy-mode rewriting: the jj/jk chords arm and
/// fire, Esc exits to Normal, and the insert-exit chord expires after its
/// timeout.
#[test]
fn test_input_state_chords_and_busy_rewriting() -> Result<()> {
    // -- Setup & Fixtures: idle Insert mode.
    let mut idle = InputState::default();
    assert_eq!(idle.mode, InputMode::Insert);

    // -- Exec & Check: jj chord arms then fires; mode is not rewritten here.
    let (action, _) = idle.handle_action(UserAction::InsertChar('j'), true);
    assert_eq!(action, UserAction::InsertChar('j'));
    let (action, _) = idle.handle_action(UserAction::InsertChar('j'), true);
    assert_eq!(action, UserAction::EnterNormalModeFromChord);
    assert_eq!(idle.mode, InputMode::Insert);

    // -- Setup & Fixtures: busy Insert mode.
    let mut busy = InputState::default();

    // -- Exec & Check: Esc exits to Normal and returns Esc.
    let (action, changed) = busy.handle_action(UserAction::Esc, false);
    assert_eq!(action, UserAction::Esc);
    assert!(changed);
    assert_eq!(busy.mode, InputMode::Normal);
    assert!(!busy.insert_exit_pending_j());

    // -- Exec & Check: the jk chord transitions to Normal.
    busy.enter_insert_mode();
    let (action, changed) = busy.handle_action(UserAction::InsertChar('j'), false);
    assert_eq!(action, UserAction::InsertChar('j'));
    assert!(changed);
    assert_eq!(busy.mode, InputMode::Insert);
    let (action, changed) = busy.handle_action(UserAction::InsertChar('k'), false);
    assert_eq!(action, UserAction::EnterNormalModeFromChord);
    assert!(changed);
    assert_eq!(busy.mode, InputMode::Normal);

    // -- Exec & Check: busy Normal 'i' enters Insert.
    let mut normal = InputState::default().with_mode(InputMode::Normal);
    let (action, changed) = normal.handle_action(UserAction::InsertChar('i'), false);
    assert_eq!(action, UserAction::Noop);
    assert!(changed);
    assert_eq!(normal.mode, InputMode::Insert);

    // -- Exec & Check: busy Visual resets chords and passes the action through.
    let mut visual = InputState::default().with_mode(InputMode::Visual);
    visual.set_insert_exit_pending_j();
    let (action, changed) = visual.handle_action(UserAction::ScrollLineDown, false);
    assert_eq!(action, UserAction::ScrollLineDown);
    assert!(!changed);
    assert!(!visual.insert_exit_pending_j());

    // -- Exec & Check: the insert-exit chord expires after its timeout.
    let mut expiring =
        InputState::default().with_insert_exit_expiry(std::time::Duration::from_millis(10));
    expiring.set_insert_exit_pending_j();
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(!expiring.insert_exit_pending_j());
    Ok(())
}

/// History navigation loads newest first, clamps at the oldest, restores the
/// saved draft, returns none for an empty history, and resets on demand; the
/// clipboard request is consumed once.
#[test]
fn test_input_state_history_and_clipboard() -> Result<()> {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    let submitted = vec!["a".to_string(), "b".to_string(), "c".to_string()];

    // -- Exec & Check: newest first, clamped at the oldest.
    assert_eq!(input.history_up(&submitted, ""), Some("c".to_string()));
    assert_eq!(input.history_up(&submitted, ""), Some("b".to_string()));
    assert_eq!(input.history_up(&submitted, ""), Some("a".to_string()));
    assert_eq!(input.history_up(&submitted, ""), Some("a".to_string()));

    // -- Setup & Fixtures: draft restore.
    let mut draft = InputState::default();
    let single = vec!["p1".to_string()];
    let up = draft
        .history_up(&single, "draft")
        .ok_or("should load history up")?;
    let down = draft.history_down(&single);

    // -- Check
    assert_eq!(up, "p1");
    assert_eq!(down, Some("draft".to_string()));

    // -- Exec & Check: empty history returns none.
    let mut empty = InputState::default();
    let none: Vec<String> = Vec::new();
    assert_eq!(empty.history_up(&none, ""), None);
    assert_eq!(empty.history_down(&none), None);

    // -- Exec & Check: reset clears the navigation index and saved draft.
    let mut reset = InputState::default();
    let _ = reset.history_up(&single, "draft");
    reset.reset_history_navigation();
    assert_eq!(reset.history_down(&single), None);

    // -- Exec & Check: the clipboard request is consumed once.
    let mut clipboard = InputState::default();
    clipboard.set_clipboard_request("payload".to_string());
    assert_eq!(
        clipboard.take_clipboard_request(),
        Some("payload".to_string())
    );
    assert_eq!(clipboard.take_clipboard_request(), None);
    Ok(())
}
