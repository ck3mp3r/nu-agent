use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalAction {
    EnableRawMode,
    DisableRawMode,
    EnterAltScreen,
    LeaveAltScreen,
    HideCursor,
    ShowCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLifecycleError {
    pub action: TerminalAction,
    pub message: String,
}

impl TerminalLifecycleError {
    pub fn new(action: TerminalAction, message: impl Into<String>) -> Self {
        Self {
            action,
            message: message.into(),
        }
    }
}

impl fmt::Display for TerminalLifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "terminal action {:?} failed: {}",
            self.action, self.message
        )
    }
}

impl Error for TerminalLifecycleError {}

pub trait TerminalBackend {
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError>;
    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError>;
    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError>;
    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError>;
    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError>;
    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TerminalState {
    raw_mode_enabled: bool,
    alt_screen_enabled: bool,
    cursor_hidden: bool,
}

pub struct TerminalLifecycle<B>
where
    B: TerminalBackend,
{
    backend: B,
    state: TerminalState,
}

impl<B> TerminalLifecycle<B>
where
    B: TerminalBackend,
{
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            state: TerminalState::default(),
        }
    }

    pub fn enter(&mut self) -> Result<(), TerminalLifecycleError> {
        if !self.state.raw_mode_enabled {
            self.backend.enable_raw_mode()?;
            self.state.raw_mode_enabled = true;
        }

        if !self.state.alt_screen_enabled {
            if let Err(error) = self.backend.enter_alt_screen() {
                let _ = self.restore();
                return Err(error);
            }
            self.state.alt_screen_enabled = true;
        }

        if !self.state.cursor_hidden {
            if let Err(error) = self.backend.hide_cursor() {
                let _ = self.restore();
                return Err(error);
            }
            self.state.cursor_hidden = true;
        }

        Ok(())
    }

    pub fn restore(&mut self) -> Result<(), TerminalLifecycleError> {
        let mut first_error: Option<TerminalLifecycleError> = None;

        if self.state.cursor_hidden {
            if let Err(error) = self.backend.show_cursor() {
                first_error.get_or_insert(error);
            } else {
                self.state.cursor_hidden = false;
            }
        }

        if self.state.alt_screen_enabled {
            if let Err(error) = self.backend.leave_alt_screen() {
                first_error.get_or_insert(error);
            } else {
                self.state.alt_screen_enabled = false;
            }
        }

        if self.state.raw_mode_enabled {
            if let Err(error) = self.backend.disable_raw_mode() {
                first_error.get_or_insert(error);
            } else {
                self.state.raw_mode_enabled = false;
            }
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
