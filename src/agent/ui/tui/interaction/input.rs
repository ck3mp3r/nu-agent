use crate::agent::ui::tui::interaction::reducer::UserAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    Char(char),
    Enter,
    AltEnter,
    ShiftEnter,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
    PageUp,
    PageDown,
    CtrlU,
    CtrlD,
    CtrlP,
    CtrlN,
    Tab,
    BackTab,
    Esc,
    CtrlC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalResize {
    pub columns: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalEvent {
    Key(TerminalKey),
    Resize(TerminalResize),
}

pub fn map_terminal_event(event: &TerminalEvent, locked: bool) -> Option<UserAction> {
    match event {
        TerminalEvent::Resize(TerminalResize { columns, rows }) => Some(UserAction::Resize {
            columns: *columns,
            rows: *rows,
        }),
        TerminalEvent::Key(key) => map_key(*key, locked),
    }
}

fn map_key(key: TerminalKey, locked: bool) -> Option<UserAction> {
    match key {
        TerminalKey::Char(_) if locked => None,
        TerminalKey::Char(ch) => Some(UserAction::InsertChar(ch)),
        TerminalKey::Enter if locked => None,
        TerminalKey::Enter => Some(UserAction::Submit),
        TerminalKey::AltEnter if locked => None,
        TerminalKey::AltEnter => Some(UserAction::InsertNewline),
        TerminalKey::ShiftEnter if locked => None,
        TerminalKey::ShiftEnter => Some(UserAction::InsertNewline),
        TerminalKey::Backspace if locked => None,
        TerminalKey::Backspace => Some(UserAction::Backspace),
        TerminalKey::Delete if locked => None,
        TerminalKey::Delete => Some(UserAction::Delete),
        TerminalKey::Left => Some(UserAction::MoveCursorLeft),
        TerminalKey::Right => Some(UserAction::MoveCursorRight),
        TerminalKey::Home => Some(UserAction::MoveCursorHome),
        TerminalKey::End => Some(UserAction::MoveCursorEnd),
        TerminalKey::Up => Some(UserAction::HistoryUp),
        TerminalKey::Down => Some(UserAction::HistoryDown),
        TerminalKey::PageUp => Some(UserAction::ScrollPageUp),
        TerminalKey::PageDown => Some(UserAction::ScrollPageDown),
        TerminalKey::CtrlU => Some(UserAction::ScrollPageUp),
        TerminalKey::CtrlD => Some(UserAction::ScrollPageDown),
        TerminalKey::CtrlP => Some(UserAction::ToggleCommandPalette),
        TerminalKey::CtrlN => Some(UserAction::QueryNext),
        TerminalKey::Tab => Some(UserAction::CompleteForward),
        TerminalKey::BackTab => Some(UserAction::CompleteBackward),
        TerminalKey::Esc => Some(UserAction::Esc),
        TerminalKey::CtrlC => Some(UserAction::Quit),
    }
}
