use std::time::Duration;

use crate::agent::ui::tui::interaction::input::{TerminalEvent, TerminalKey};

use super::terminal_io::TtyTerminalEvents;

pub trait TerminalEventSource {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String>;

    fn diagnostics(&self) -> InputSourceDiagnostics {
        InputSourceDiagnostics::unknown()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSourceDiagnostics {
    pub active_backend: &'static str,
    pub primary_available: Option<bool>,
    pub fallback_available: Option<bool>,
    pub last_poll_state: String,
    pub last_error: Option<String>,
}

impl InputSourceDiagnostics {
    fn unknown() -> Self {
        Self {
            active_backend: "unknown",
            primary_available: None,
            fallback_available: None,
            last_poll_state: "waiting for input poll".to_string(),
            last_error: None,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ScriptedTerminalEvents {
    queue: std::collections::VecDeque<TerminalEvent>,
}

#[cfg(test)]
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

#[cfg(test)]
impl TerminalEventSource for ScriptedTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        Ok(self.queue.pop_front())
    }
}

#[derive(Debug, Clone)]
pub struct CrosstermTerminalEvents {
    poll_timeout: Duration,
}

impl Default for CrosstermTerminalEvents {
    fn default() -> Self {
        Self::new(Duration::from_millis(60))
    }
}

impl CrosstermTerminalEvents {
    pub fn new(poll_timeout: Duration) -> Self {
        Self { poll_timeout }
    }
}

impl TerminalEventSource for CrosstermTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        let ready = crossterm::event::poll(self.poll_timeout)
            .map_err(|err| format!("crossterm poll failed: {err}"))?;
        if !ready {
            return Ok(None);
        }

        let event = crossterm::event::read().map_err(|err| format!("crossterm read failed: {err}"))?;
        Ok(map_crossterm_event(event))
    }
}

#[derive(Debug)]
pub struct HybridTerminalEvents {
    primary: CrosstermTerminalEvents,
    fallback: Option<TtyTerminalEvents>,
    diagnostics: InputSourceDiagnostics,
}

impl HybridTerminalEvents {
    pub fn new(poll_timeout: Duration, fallback: Option<TtyTerminalEvents>) -> Self {
        let fallback_available = Some(fallback.is_some());
        Self {
            primary: CrosstermTerminalEvents::new(poll_timeout),
            fallback,
            diagnostics: InputSourceDiagnostics {
                active_backend: "none",
                primary_available: Some(true),
                fallback_available,
                last_poll_state: "not polled yet".to_string(),
                last_error: None,
            },
        }
    }
}

fn poll_hybrid_event<P, F>(
    primary: &mut P,
    mut fallback: Option<&mut F>,
    diagnostics: &mut InputSourceDiagnostics,
    prefix_fallback_idle_error: bool,
) -> Result<Option<TerminalEvent>, String>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    match primary.poll_event() {
        Ok(Some(event)) => {
            diagnostics.active_backend = "crossterm";
            diagnostics.primary_available = Some(true);
            diagnostics.last_poll_state = "crossterm delivered event".to_string();
            diagnostics.last_error = None;
            Ok(Some(event))
        }
        Ok(None) => match fallback.as_mut() {
            Some(fallback) => match fallback.poll_event() {
                Ok(Some(event)) => {
                    diagnostics.active_backend = "tty";
                    diagnostics.fallback_available = Some(true);
                    diagnostics.last_poll_state = "/dev/tty delivered event".to_string();
                    diagnostics.last_error = None;
                    Ok(Some(event))
                }
                Ok(None) => {
                    diagnostics.active_backend = "none";
                    diagnostics.last_poll_state = "crossterm idle; /dev/tty idle".to_string();
                    Ok(None)
                }
                Err(fallback_error) => {
                    diagnostics.active_backend = "none";
                    diagnostics.fallback_available = Some(false);
                    diagnostics.last_poll_state = "crossterm idle; /dev/tty error".to_string();
                    diagnostics.last_error = Some(fallback_error.clone());
                    if prefix_fallback_idle_error {
                        Err(format!("tty fallback failed: {fallback_error}"))
                    } else {
                        Err(fallback_error)
                    }
                }
            },
            None => {
                diagnostics.active_backend = "none";
                diagnostics.fallback_available = Some(false);
                diagnostics.last_poll_state = "crossterm idle; /dev/tty unavailable".to_string();
                Ok(None)
            }
        },
        Err(primary_error) => match fallback.as_mut() {
            Some(fallback) => match fallback.poll_event() {
                Ok(Some(event)) => {
                    diagnostics.active_backend = "tty";
                    diagnostics.primary_available = Some(false);
                    diagnostics.fallback_available = Some(true);
                    diagnostics.last_poll_state =
                        "crossterm error; /dev/tty delivered event".to_string();
                    diagnostics.last_error = Some(primary_error);
                    Ok(Some(event))
                }
                Ok(None) => {
                    diagnostics.active_backend = "none";
                    diagnostics.primary_available = Some(false);
                    diagnostics.last_poll_state = "crossterm error; /dev/tty idle".to_string();
                    diagnostics.last_error = Some(primary_error.clone());
                    Ok(None)
                }
                Err(fallback_error) => {
                    diagnostics.active_backend = "none";
                    diagnostics.primary_available = Some(false);
                    diagnostics.fallback_available = Some(false);
                    diagnostics.last_poll_state = "crossterm error; /dev/tty error".to_string();
                    diagnostics.last_error =
                        Some(format!("{primary_error}; tty fallback failed: {fallback_error}"));
                    Err(format!("{primary_error}; tty fallback failed: {fallback_error}"))
                }
            },
            None => {
                diagnostics.active_backend = "none";
                diagnostics.primary_available = Some(false);
                diagnostics.fallback_available = Some(false);
                diagnostics.last_poll_state = "crossterm error; /dev/tty unavailable".to_string();
                diagnostics.last_error = Some(primary_error.clone());
                Err(primary_error)
            }
        },
    }
}

#[cfg(test)]
pub(crate) struct HybridTerminalEventsForTest<P, F>
where
    P: TerminalEventSource,
    F: TerminalEventSource,
{
    primary: P,
    fallback: F,
    diagnostics: InputSourceDiagnostics,
}

#[cfg(test)]
impl HybridTerminalEvents {
    pub(crate) fn new_for_test<P, F>(
        primary: P,
        fallback: F,
    ) -> HybridTerminalEventsForTest<P, F>
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

#[cfg(test)]
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

impl TerminalEventSource for HybridTerminalEvents {
    fn poll_event(&mut self) -> Result<Option<TerminalEvent>, String> {
        poll_hybrid_event(
            &mut self.primary,
            self.fallback.as_mut(),
            &mut self.diagnostics,
            true,
        )
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

fn map_crossterm_event(event: crossterm::event::Event) -> Option<TerminalEvent> {
    use crossterm::event::{
        Event,
        KeyCode,
        KeyEventKind,
        KeyModifiers,
    };

    match event {
        Event::Resize(columns, rows) => {
            Some(TerminalEvent::Resize(
                crate::agent::ui::tui::interaction::input::TerminalResize {
                columns,
                rows,
                },
            ))
        }
        Event::Key(key_event) => {
            if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
                return None;
            }

            let key = match key_event.code {
                KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlC
                }
                KeyCode::Char('u') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlU
                }
                KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlD
                }
                KeyCode::Char('p') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlP
                }
                KeyCode::Char('n') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    TerminalKey::CtrlN
                }
                KeyCode::Char(ch) => TerminalKey::Char(ch),
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::ALT) => {
                    TerminalKey::AltEnter
                }
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                    TerminalKey::ShiftEnter
                }
                KeyCode::Enter => TerminalKey::Enter,
                KeyCode::Backspace => TerminalKey::Backspace,
                KeyCode::Delete => TerminalKey::Delete,
                KeyCode::Left => TerminalKey::Left,
                KeyCode::Right => TerminalKey::Right,
                KeyCode::Home => TerminalKey::Home,
                KeyCode::End => TerminalKey::End,
                KeyCode::Up => TerminalKey::Up,
                KeyCode::Down => TerminalKey::Down,
                KeyCode::PageUp => TerminalKey::PageUp,
                KeyCode::PageDown => TerminalKey::PageDown,
                KeyCode::Tab => TerminalKey::Tab,
                KeyCode::BackTab => TerminalKey::BackTab,
                KeyCode::Esc => TerminalKey::Esc,
                _ => return None,
            };

            Some(TerminalEvent::Key(key))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn map_crossterm_event_for_test(event: crossterm::event::Event) -> Option<TerminalEvent> {
    map_crossterm_event(event)
}

#[cfg(test)]
fn parse_script_token(token: &str) -> Option<TerminalEvent> {
    use crate::agent::ui::tui::interaction::input::TerminalResize;

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

    None
}
