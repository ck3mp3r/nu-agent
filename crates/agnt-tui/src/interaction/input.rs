//! The terminal boundary: raw terminal events in, surface input out.
//!
//! Input is surface state, not bus traffic: a key press, a wheel notch, and a
//! resize are interpreted here and never leave this crate.

/// A key the reducer interprets. The surface owns this enum so no terminal
/// library type crosses the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
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
    CtrlJ,
    CtrlK,
    Tab,
    BackTab,
    Esc,
    CtrlC,
}

/// One input the surface interprets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiInput {
    KeyPressed { key: Key },
    WheelScrolled { up: bool, column: u16, row: u16 },
    Resized { width: u16, height: u16 },
}

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
    CtrlJ,
    CtrlK,
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
pub struct TerminalMouse {
    pub up: bool,
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    Key(TerminalKey),
    Resize(TerminalResize),
    Paste(String),
    Mouse(TerminalMouse),
}

/// Translate one terminal event into the input the surface interprets. The
/// terminal layer reports keys, wheel notches, and resizes; what a key means
/// (the keymap, the input lock, the open card) belongs to the reducer.
pub fn map_terminal_event(event: &TerminalEvent) -> Option<UiInput> {
    match event {
        TerminalEvent::Resize(TerminalResize { columns, rows }) => Some(UiInput::Resized {
            width: *columns,
            height: *rows,
        }),
        TerminalEvent::Key(key) => Some(UiInput::KeyPressed { key: key.to_core() }),
        TerminalEvent::Paste(_) => None,
        TerminalEvent::Mouse(mouse) => Some(UiInput::WheelScrolled {
            up: mouse.up,
            column: mouse.column,
            row: mouse.row,
        }),
    }
}

impl TerminalKey {
    /// The core key this terminal key stands for.
    pub(crate) fn to_core(self) -> Key {
        match self {
            TerminalKey::Char(ch) => Key::Char(ch),
            TerminalKey::Enter => Key::Enter,
            TerminalKey::AltEnter => Key::AltEnter,
            TerminalKey::ShiftEnter => Key::ShiftEnter,
            TerminalKey::Backspace => Key::Backspace,
            TerminalKey::Delete => Key::Delete,
            TerminalKey::Left => Key::Left,
            TerminalKey::Right => Key::Right,
            TerminalKey::Home => Key::Home,
            TerminalKey::End => Key::End,
            TerminalKey::Up => Key::Up,
            TerminalKey::Down => Key::Down,
            TerminalKey::PageUp => Key::PageUp,
            TerminalKey::PageDown => Key::PageDown,
            TerminalKey::CtrlU => Key::CtrlU,
            TerminalKey::CtrlD => Key::CtrlD,
            TerminalKey::CtrlP => Key::CtrlP,
            TerminalKey::CtrlN => Key::CtrlN,
            TerminalKey::CtrlJ => Key::CtrlJ,
            TerminalKey::CtrlK => Key::CtrlK,
            TerminalKey::Tab => Key::Tab,
            TerminalKey::BackTab => Key::BackTab,
            TerminalKey::Esc => Key::Esc,
            TerminalKey::CtrlC => Key::CtrlC,
        }
    }
}
