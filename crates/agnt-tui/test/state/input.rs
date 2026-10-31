//! Unit tests for the input domain: mode transitions, chords, history, and
//! the clipboard request.

use super::*;
use crate::interaction::reducer::reduce;
use agnt_core::protocol::events::{Key, SystemEvent};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Press one core key through the reducer.
fn press(state: &mut AppState, key: Key) -> bool {
    reduce(state, SystemEvent::KeyPressed { key }, None)
}

/// An idle Normal-mode state with a scrollable transcript.
fn normal_state() -> AppState {
    AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            pane_focus: PaneFocus::Transcript,
            viewport_height: 10,
            cursor_visual_row: 5,
            total_visual_rows: 20,
            ..Default::default()
        },
        ..Default::default()
    }
}

// endregion: --- Test Support

/// Idle Normal-mode keymap: the vim keys drive the mode, focus, and scroll
/// transitions, and unmapped characters are no-ops.
#[test]
fn test_input_state_idle_normal_keymap_matrix() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = normal_state();

    // -- Exec & Check: the line moves walk the cursor.
    assert!(press(&mut state, Key::Char('j')));
    assert_eq!(state.scroll.cursor_visual_row, 6, "j moves down");
    assert!(press(&mut state, Key::Char('k')));
    assert_eq!(state.scroll.cursor_visual_row, 5, "k moves up");

    // -- Exec & Check: h and l steer the pane focus.
    press(&mut state, Key::Char('h'));
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input, "h focuses input");
    press(&mut state, Key::Char('l'));
    assert_eq!(
        state.scroll.pane_focus,
        PaneFocus::Transcript,
        "l focuses the transcript"
    );

    // -- Exec & Check: the completion keys move the focus the same way. The TUI
    // has two panes, so left and right both toggle them.
    press(&mut state, Key::Tab);
    assert_eq!(
        state.scroll.pane_focus,
        PaneFocus::Input,
        "Tab leaves the transcript"
    );
    press(&mut state, Key::BackTab);
    assert_eq!(
        state.scroll.pane_focus,
        PaneFocus::Transcript,
        "BackTab returns to the transcript"
    );

    // -- Exec & Check: G jumps to the last visual row.
    press(&mut state, Key::Char('G'));
    assert_eq!(state.scroll.cursor_visual_row, 19, "G jumps to the bottom");

    // -- Exec & Check: an unmapped character changes nothing.
    assert!(!press(&mut state, Key::Char('x')));
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec & Check: Escape passes through while idle: nothing to abort.
    assert!(!press(&mut state, Key::Esc));
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec & Check: the gg chord arms then fires.
    let mut chord = normal_state();
    assert!(!press(&mut chord, Key::Char('g')), "the first g arms");
    assert_eq!(chord.scroll.cursor_visual_row, 5);
    assert!(press(&mut chord, Key::Char('g')), "the second g fires");
    assert_eq!(chord.scroll.cursor_visual_row, 0, "gg jumps to the top");

    // -- Exec & Check: i and v change the mode, y yanks.
    let mut modes = normal_state();
    press(&mut modes, Key::Char('i'));
    assert_eq!(modes.input.mode, InputMode::Insert, "i enters insert mode");

    let mut visual = normal_state();
    press(&mut visual, Key::Char('v'));
    assert_eq!(visual.input.mode, InputMode::Visual, "v enters visual mode");
    assert!(press(&mut visual, Key::Char('y')));
    assert_eq!(visual.input.mode, InputMode::Normal, "y leaves visual mode");
    Ok(())
}

/// Insert-mode chords and the busy-mode rewriting: the jj/jk chords arm and
/// fire, Esc exits to Normal, and the insert-exit chord expires after its
/// timeout.
#[test]
fn test_input_state_chords_and_busy_rewriting() -> Result<()> {
    // -- Setup & Fixtures: idle Insert mode.
    let mut idle = AppState::default();
    assert_eq!(idle.input.mode, InputMode::Insert);

    // -- Exec & Check: jj chord arms then fires.
    assert!(
        !press(&mut idle, Key::Char('j')),
        "the armed chord is silent"
    );
    assert_eq!(idle.input.mode, InputMode::Insert);
    assert!(press(&mut idle, Key::Char('j')), "the chord fires");
    assert_eq!(idle.input.mode, InputMode::Normal);

    // -- Setup & Fixtures: a running turn in Insert mode.
    let mut busy = AppState {
        phase: UiPhase::Busy,
        ..Default::default()
    };

    // -- Exec & Check: Esc exits to Normal.
    assert!(press(&mut busy, Key::Esc));
    assert_eq!(busy.input.mode, InputMode::Normal);
    assert!(!busy.input.insert_exit_pending_j());

    // -- Exec & Check: the jk chord transitions to Normal.
    busy.enter_insert_mode();
    assert!(press(&mut busy, Key::Char('j')), "the busy chord repaints");
    assert_eq!(busy.input.mode, InputMode::Insert);
    assert!(press(&mut busy, Key::Char('k')), "the chord fires");
    assert_eq!(busy.input.mode, InputMode::Normal);

    // -- Exec & Check: busy Normal `i` enters Insert.
    let mut normal = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        phase: UiPhase::Busy,
        ..Default::default()
    };
    assert!(press(&mut normal, Key::Char('i')));
    assert_eq!(normal.input.mode, InputMode::Insert);

    // -- Exec & Check: busy Visual resets chords and passes the key through.
    let mut visual = AppState {
        input: InputState::default().with_mode(InputMode::Visual),
        phase: UiPhase::Busy,
        ..Default::default()
    };
    visual.input.set_insert_exit_pending_j();
    assert!(!press(&mut visual, Key::Char('j')));
    assert_eq!(visual.input.mode, InputMode::Visual);
    assert!(!visual.input.insert_exit_pending_j());

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
