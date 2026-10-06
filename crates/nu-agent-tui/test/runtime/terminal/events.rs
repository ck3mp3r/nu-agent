use crate::interaction::input::{TerminalEvent, TerminalKey, TerminalResize};

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
