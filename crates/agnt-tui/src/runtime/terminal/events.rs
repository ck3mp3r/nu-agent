use std::time::Duration;

use crate::interaction::input::{TerminalEvent, TerminalKey};

use super::io::TtyTerminalEvents;

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

        let event =
            crossterm::event::read().map_err(|err| format!("crossterm read failed: {err}"))?;
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

pub(crate) fn poll_hybrid_event<P, F>(
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
                    diagnostics.last_error = Some(format!(
                        "{primary_error}; tty fallback failed: {fallback_error}"
                    ));
                    Err(format!(
                        "{primary_error}; tty fallback failed: {fallback_error}"
                    ))
                }
            },
            None => {
                diagnostics.active_backend = "none";
                diagnostics.primary_available = Some(false);
                diagnostics.fallback_available = Some(false);
                diagnostics.last_poll_state = "crossterm error; /dev/tty unavailable".to_string();
                diagnostics.last_error = Some(primary_error);
                Ok(None)
            }
        },
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

pub(crate) fn map_crossterm_event(event: crossterm::event::Event) -> Option<TerminalEvent> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

    match event {
        Event::Resize(columns, rows) => Some(TerminalEvent::Resize(
            crate::interaction::input::TerminalResize { columns, rows },
        )),
        Event::Paste(text) => Some(TerminalEvent::Paste(text)),
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
