use crate::interaction::input::{TerminalEvent, TerminalKey, TerminalMouse, TerminalResize};

use super::TerminalEventSource;
use super::{InputSourceDiagnostics, map_crossterm_event, poll_hybrid_event};

#[derive(Debug, Clone)]
pub struct ScriptedTerminalEvents {
    queue: std::collections::VecDeque<TerminalEvent>,
}

impl ScriptedTerminalEvents {
    pub fn from_script(script: &str) -> Self {
        let mut queue = std::collections::VecDeque::new();

        for raw in script.split(',') {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }

            if let Some(event) = parse_script_token(token) {
                queue.push_back(event);
            }
        }

        Self { queue }
    }
}

impl TerminalEventSource for ScriptedTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Ok(self.queue.pop_front())
    }
}

pub(crate) struct HybridTerminalEventsForTest<P, F>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    primary: P,
    fallback: F,
    diagnostics: InputSourceDiagnostics,
}

impl super::HybridTerminalEvents {
    pub(crate) fn new_for_test<P, F>(primary: P, fallback: F) -> HybridTerminalEventsForTest<P, F>
    where
        P: TerminalEventSource,
        F: TerminalEventSource,
    {
        HybridTerminalEventsForTest {
            primary,
            fallback,
            diagnostics: InputSourceDiagnostics {
                active_backend: "none",
                primary_available: Some(true),
                fallback_available: Some(true),
                last_poll_state: "not polled yet".to_string(),
                last_error: None,
            },
        }
    }
}

impl<P, F> TerminalEventSource for HybridTerminalEventsForTest<P, F>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        poll_hybrid_event(
            &mut self.primary,
            Some(&mut self.fallback),
            &mut self.diagnostics,
            false,
        )
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

pub(crate) fn map_crossterm_event_for_test(
    event: crossterm::event::Event,
) -> Option<TerminalEvent> {
    map_crossterm_event(event)
}

fn parse_script_token(token: &str) -> Option<TerminalEvent> {
    let lower = token.to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" => Some(TerminalKey::Enter),
        "backspace" => Some(TerminalKey::Backspace),
        "delete" => Some(TerminalKey::Delete),
        "left" => Some(TerminalKey::Left),
        "right" => Some(TerminalKey::Right),
        "home" => Some(TerminalKey::Home),
        "end" => Some(TerminalKey::End),
        "up" => Some(TerminalKey::Up),
        "down" => Some(TerminalKey::Down),
        "pgup" | "pageup" => Some(TerminalKey::PageUp),
        "pgdown" | "pagedown" => Some(TerminalKey::PageDown),
        "tab" => Some(TerminalKey::Tab),
        "backtab" => Some(TerminalKey::BackTab),
        "esc" => Some(TerminalKey::Esc),
        "ctrlc" => Some(TerminalKey::CtrlC),
        "ctrlu" => Some(TerminalKey::CtrlU),
        "ctrld" => Some(TerminalKey::CtrlD),
        "ctrlp" => Some(TerminalKey::CtrlP),
        "ctrln" => Some(TerminalKey::CtrlN),
        _ => None,
    };

    if let Some(key) = key {
        return Some(TerminalEvent::Key(key));
    }

    if let Some(chars) = token.strip_prefix("char:") {
        return chars
            .chars()
            .next()
            .map(TerminalKey::Char)
            .map(TerminalEvent::Key);
    }

    if let Some(size) = token.strip_prefix("resize:")
        && let Some((columns, rows)) = size.split_once('x')
        && let (Ok(columns), Ok(rows)) = (columns.parse::<u16>(), rows.parse::<u16>())
    {
        return Some(TerminalEvent::Resize(TerminalResize { columns, rows }));
    }

    if let Some(text) = token.strip_prefix("paste:") {
        return Some(TerminalEvent::Paste(text.to_string()));
    }

    None
}

#[test]
fn scripted_event_parser_supports_keys_chars_and_resize() {
    let mut source =
        ScriptedTerminalEvents::from_script("char:a,enter,esc,resize:120x40,ctrlu,ctrld,ctrlc");

    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Char('a'))))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Enter)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::Esc)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Resize(
            crate::interaction::input::TerminalResize {
                columns: 120,
                rows: 40,
            }
        )))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlU)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlD)))
    );
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlC)))
    );
    assert_eq!(source.poll_event(), Ok(None));
}

#[test]
fn scripted_event_parser_supports_ctrlp_for_palette_toggle() {
    let mut source = ScriptedTerminalEvents::from_script("ctrlp");
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlP)))
    );
}

#[test]
fn scripted_event_parser_supports_ctrln_for_query_picker_navigation() {
    let mut source = ScriptedTerminalEvents::from_script("ctrln");
    assert_eq!(
        source.poll_event(),
        Ok(Some(TerminalEvent::Key(TerminalKey::CtrlN)))
    );
}

#[test]
fn test_map_crossterm_event_control_j_and_k_keep_their_modifier() {
    // -- Setup & Fixtures: crossterm delivers Ctrl-J/Ctrl-K as Char + CONTROL.
    let control_cases = [('j', TerminalKey::CtrlJ), ('k', TerminalKey::CtrlK)];
    let plain_cases = [('j', TerminalKey::Char('j')), ('k', TerminalKey::Char('k'))];

    // -- Exec & Check: with CONTROL they are named navigation keys.
    for (ch, expected) in control_cases {
        let mapped = map_crossterm_event_for_test(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        ));
        assert_eq!(mapped, Some(TerminalEvent::Key(expected)), "ctrl-{ch}");
    }

    // -- Check: without CONTROL they stay plain characters (type-to-filter).
    for (ch, expected) in plain_cases {
        let mapped = map_crossterm_event_for_test(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ),
        ));
        assert_eq!(mapped, Some(TerminalEvent::Key(expected)), "plain {ch}");
    }
}

#[test]
fn test_map_crossterm_event_mouse_wheel_maps_direction_and_position() {
    // -- Setup & Fixtures
    let scroll_up = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollUp,
        column: 12,
        row: 7,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });
    let scroll_down = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::ScrollDown,
        column: 3,
        row: 21,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });

    // -- Exec
    let up = map_crossterm_event_for_test(scroll_up);
    let down = map_crossterm_event_for_test(scroll_down);

    // -- Check
    assert_eq!(
        up,
        Some(TerminalEvent::Mouse(TerminalMouse {
            up: true,
            column: 12,
            row: 7,
        }))
    );
    assert_eq!(
        down,
        Some(TerminalEvent::Mouse(TerminalMouse {
            up: false,
            column: 3,
            row: 21,
        }))
    );
}

#[test]
fn test_map_crossterm_event_mouse_non_wheel_kinds_are_dropped() {
    // -- Setup & Fixtures
    let kinds = [
        crossterm::event::MouseEventKind::Moved,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::ScrollLeft,
        crossterm::event::MouseEventKind::ScrollRight,
    ];

    // -- Exec & Check
    for kind in kinds {
        let mapped = map_crossterm_event_for_test(crossterm::event::Event::Mouse(
            crossterm::event::MouseEvent {
                kind,
                column: 5,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        ));
        assert_eq!(mapped, None, "kind {kind:?} must be dropped");
    }
}

fn mapped_key(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> Option<TerminalEvent> {
    map_crossterm_event_for_test(crossterm::event::Event::Key(
        crossterm::event::KeyEvent::new(code, modifiers),
    ))
}

#[test]
fn test_map_crossterm_event_unbound_modifier_chars_are_inert() {
    // -- Setup & Fixtures: every covered modifier on a letter no arm binds.
    let cases = [
        (
            crossterm::event::KeyCode::Char('f'),
            crossterm::event::KeyModifiers::CONTROL,
            "ctrl-f",
        ),
        (
            crossterm::event::KeyCode::Char('v'),
            crossterm::event::KeyModifiers::CONTROL,
            "ctrl-v",
        ),
        (
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
            "ctrl-a",
        ),
        (
            crossterm::event::KeyCode::Char('b'),
            crossterm::event::KeyModifiers::ALT,
            "alt-b",
        ),
        (
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::SUPER,
            "super-a",
        ),
        (
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::HYPER,
            "hyper-x",
        ),
        (
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::META,
            "meta-y",
        ),
        (
            crossterm::event::KeyCode::Char('f'),
            crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::SHIFT,
            "ctrl-shift-f",
        ),
    ];

    // -- Exec & Check
    for (code, modifiers, label) in cases {
        assert_eq!(mapped_key(code, modifiers), None, "{label} must not paint");
    }
}

#[test]
fn test_map_crossterm_event_bound_control_chars_keep_their_bindings() {
    // -- Setup & Fixtures: the seven bound control letters.
    let cases = [
        ('c', TerminalKey::CtrlC, "ctrl-c"),
        ('u', TerminalKey::CtrlU, "ctrl-u"),
        ('d', TerminalKey::CtrlD, "ctrl-d"),
        ('p', TerminalKey::CtrlP, "ctrl-p"),
        ('n', TerminalKey::CtrlN, "ctrl-n"),
        ('j', TerminalKey::CtrlJ, "ctrl-j"),
        ('k', TerminalKey::CtrlK, "ctrl-k"),
    ];
    let control = crossterm::event::KeyModifiers::CONTROL;
    let control_shift = control | crossterm::event::KeyModifiers::SHIFT;

    // -- Exec & Check: CONTROL and its SHIFT superset keep the binding.
    for (ch, expected, label) in cases {
        assert_eq!(
            mapped_key(crossterm::event::KeyCode::Char(ch), control),
            Some(TerminalEvent::Key(expected)),
            "{label}"
        );
        assert_eq!(
            mapped_key(crossterm::event::KeyCode::Char(ch), control_shift),
            Some(TerminalEvent::Key(expected)),
            "{label} with shift"
        );
    }
}

#[test]
fn test_map_crossterm_event_shift_and_unmodified_chars_still_type() {
    // -- Setup & Fixtures: capitals arrive as Char with SHIFT only.
    let cases = [
        ('A', crossterm::event::KeyModifiers::SHIFT, "shift-a"),
        ('Z', crossterm::event::KeyModifiers::SHIFT, "shift-z"),
        ('a', crossterm::event::KeyModifiers::NONE, "plain-a"),
        ('7', crossterm::event::KeyModifiers::NONE, "plain-7"),
    ];

    // -- Exec & Check
    for (ch, modifiers, label) in cases {
        assert_eq!(
            mapped_key(crossterm::event::KeyCode::Char(ch), modifiers),
            Some(TerminalEvent::Key(TerminalKey::Char(ch))),
            "{label} must still type"
        );
    }
}

#[test]
fn test_map_crossterm_event_enter_variants_unchanged() {
    // -- Setup & Fixtures: the three Enter forms.
    let cases = [
        (
            crossterm::event::KeyModifiers::ALT,
            TerminalKey::AltEnter,
            "alt-enter",
        ),
        (
            crossterm::event::KeyModifiers::SHIFT,
            TerminalKey::ShiftEnter,
            "shift-enter",
        ),
        (
            crossterm::event::KeyModifiers::NONE,
            TerminalKey::Enter,
            "enter",
        ),
    ];

    // -- Exec & Check
    for (modifiers, expected, label) in cases {
        assert_eq!(
            mapped_key(crossterm::event::KeyCode::Enter, modifiers),
            Some(TerminalEvent::Key(expected)),
            "{label}"
        );
    }
}

#[test]
fn test_map_crossterm_event_bracketed_paste_unchanged() {
    // -- Setup & Fixtures: bracketed paste is a separate event kind.
    let event = crossterm::event::Event::Paste("hello".to_string());

    // -- Exec
    let mapped = map_crossterm_event_for_test(event);

    // -- Check
    assert_eq!(mapped, Some(TerminalEvent::Paste("hello".to_string())));
}
