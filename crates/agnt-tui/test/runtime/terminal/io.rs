use super::TerminalEvent;
use crate::interaction::input::TerminalKey;

/// The tty fallback sees raw control bytes: 0x0A/0x0B are Ctrl-J/Ctrl-K, not
/// Enter, and CR stays the submit key.
#[test]
fn test_map_tty_byte_control_bytes_map_to_named_keys() {
    // -- Setup & Fixtures
    let cases = [
        (3u8, Some(TerminalKey::CtrlC)),
        (b'\r', Some(TerminalKey::Enter)),
        (b'\n', Some(TerminalKey::CtrlJ)),
        (11, Some(TerminalKey::CtrlK)),
        (8, Some(TerminalKey::Backspace)),
        (127, Some(TerminalKey::Backspace)),
        (27, Some(TerminalKey::Esc)),
        (b'j', Some(TerminalKey::Char('j'))),
        (b'k', Some(TerminalKey::Char('k'))),
        (b' ', Some(TerminalKey::Char(' '))),
        (0, None),
        (1, None),
    ];

    // -- Exec & Check
    for (byte, expected) in cases {
        let mapped = super::map_tty_byte(byte);
        let expected = expected.map(TerminalEvent::Key);
        assert_eq!(mapped, expected, "byte {byte:#04x}");
    }
}
