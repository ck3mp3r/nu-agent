use crate::agent::ui::tui::{
    interaction::{
        input::{map_terminal_event, TerminalEvent, TerminalKey, TerminalResize},
        reducer::UserAction,
    },
};

#[derive(Debug)]
struct MappingCase {
    event: TerminalEvent,
    locked: bool,
    expected: Option<UserAction>,
}

#[test]
fn table_driven_key_mapping_unlocked_state() {
    let cases = vec![
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Char('a')),
            locked: false,
            expected: Some(UserAction::InsertChar('a')),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Enter),
            locked: false,
            expected: Some(UserAction::Submit),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::AltEnter),
            locked: false,
            expected: Some(UserAction::InsertNewline),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::ShiftEnter),
            locked: false,
            expected: Some(UserAction::InsertNewline),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Backspace),
            locked: false,
            expected: Some(UserAction::Backspace),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Delete),
            locked: false,
            expected: Some(UserAction::Delete),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Left),
            locked: false,
            expected: Some(UserAction::MoveCursorLeft),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Right),
            locked: false,
            expected: Some(UserAction::MoveCursorRight),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Home),
            locked: false,
            expected: Some(UserAction::MoveCursorHome),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::End),
            locked: false,
            expected: Some(UserAction::MoveCursorEnd),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Up),
            locked: false,
            expected: Some(UserAction::HistoryUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Down),
            locked: false,
            expected: Some(UserAction::HistoryDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::PageUp),
            locked: false,
            expected: Some(UserAction::ScrollPageUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::PageDown),
            locked: false,
            expected: Some(UserAction::ScrollPageDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlU),
            locked: false,
            expected: Some(UserAction::ScrollPageUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlD),
            locked: false,
            expected: Some(UserAction::ScrollPageDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Tab),
            locked: false,
            expected: Some(UserAction::CompleteForward),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::BackTab),
            locked: false,
            expected: Some(UserAction::CompleteBackward),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Esc),
            locked: false,
            expected: Some(UserAction::Esc),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlC),
            locked: false,
            expected: Some(UserAction::Quit),
        },
    ];

    for case in cases {
        let actual = map_terminal_event(&case.event, case.locked);
        assert_eq!(actual, case.expected);
    }
}

#[test]
fn table_driven_key_mapping_locked_state_blocks_mutation_and_submit_only() {
    let blocked_cases = vec![
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Char('a')),
            locked: true,
            expected: None,
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Enter),
            locked: true,
            expected: None,
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::AltEnter),
            locked: true,
            expected: None,
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::ShiftEnter),
            locked: true,
            expected: None,
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Backspace),
            locked: true,
            expected: None,
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Delete),
            locked: true,
            expected: None,
        },
    ];

    for case in blocked_cases {
        let actual = map_terminal_event(&case.event, case.locked);
        assert_eq!(actual, case.expected);
    }

    let allowed_cases = vec![
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Esc),
            locked: true,
            expected: Some(UserAction::Esc),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlC),
            locked: true,
            expected: Some(UserAction::Quit),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Left),
            locked: true,
            expected: Some(UserAction::MoveCursorLeft),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Right),
            locked: true,
            expected: Some(UserAction::MoveCursorRight),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Home),
            locked: true,
            expected: Some(UserAction::MoveCursorHome),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::End),
            locked: true,
            expected: Some(UserAction::MoveCursorEnd),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Up),
            locked: true,
            expected: Some(UserAction::HistoryUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Down),
            locked: true,
            expected: Some(UserAction::HistoryDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::PageUp),
            locked: true,
            expected: Some(UserAction::ScrollPageUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::PageDown),
            locked: true,
            expected: Some(UserAction::ScrollPageDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlU),
            locked: true,
            expected: Some(UserAction::ScrollPageUp),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::CtrlD),
            locked: true,
            expected: Some(UserAction::ScrollPageDown),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::Tab),
            locked: true,
            expected: Some(UserAction::CompleteForward),
        },
        MappingCase {
            event: TerminalEvent::Key(TerminalKey::BackTab),
            locked: true,
            expected: Some(UserAction::CompleteBackward),
        },
    ];

    for case in allowed_cases {
        let actual = map_terminal_event(&case.event, case.locked);
        assert_eq!(actual, case.expected);
    }
}

#[test]
fn resize_event_maps_to_user_action_with_dimensions() {
    let event = TerminalEvent::Resize(TerminalResize {
        columns: 120,
        rows: 42,
    });

    let actual = map_terminal_event(&event, false);

    assert_eq!(actual, Some(UserAction::Resize { columns: 120, rows: 42 }));
}
