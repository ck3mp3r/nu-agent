use super::*;
use std::future::Future;

#[derive(Debug)]
pub enum RuntimeRunError<E> {
    Enter(TerminalLifecycleError),
    Run(RestoreRunError<E, TerminalLifecycleError>),
}

pub fn run_with_terminal_restore_sync<B, T, E, F>(
    lifecycle: &mut TerminalLifecycle<B>,
    run: F,
) -> Result<T, RuntimeRunError<E>>
where
    B: TerminalBackend,
    F: FnOnce() -> Result<T, E>,
{
    lifecycle.enter().map_err(RuntimeRunError::Enter)?;
    run_with_restore(lifecycle, run).map_err(RuntimeRunError::Run)
}

pub async fn run_with_terminal_restore<B, T, E, F, Fut>(
    lifecycle: &mut TerminalLifecycle<B>,
    run: F,
) -> Result<T, RuntimeRunError<E>>
where
    B: TerminalBackend,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    lifecycle.enter().map_err(RuntimeRunError::Enter)?;
    let run_result = run().await;
    let _ = lifecycle.restore();
    run_result.map_err(|e| RuntimeRunError::Run(RestoreRunError::Run(e)))
}

pub struct AnsiTerminalBackend<W>
where
    W: Write,
{
    writer: W,
}

impl<W> AnsiTerminalBackend<W>
where
    W: Write,
{
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> TerminalBackend for AnsiTerminalBackend<W>
where
    W: Write,
{
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::enable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::EnableRawMode,
                err.to_string(),
            )
        })
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::disable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::DisableRawMode,
                err.to_string(),
            )
        })
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::EnterAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::EnterAltScreen,
                err.to_string(),
            )
        })
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::LeaveAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::LeaveAltScreen,
                err.to_string(),
            )
        })
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Hide).map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::HideCursor,
                err.to_string(),
            )
        })
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Show).map_err(|err| {
            TerminalLifecycleError::new(
                crate::platform::terminal::TerminalAction::ShowCursor,
                err.to_string(),
            )
        })
    }
}

pub(super) struct LiveTerminalUi {
    pub(super) terminal: Terminal<CrosstermBackend<std::io::Stderr>>,
}

impl LiveTerminalUi {
    pub(super) fn new() -> Result<Self, String> {
        let backend = CrosstermBackend::new(std::io::stderr());
        let mut terminal = Terminal::new(backend)
            .map_err(|err| format!("failed to initialize ratatui terminal: {err}"))?;
        terminal
            .clear()
            .map_err(|err| format!("failed to clear ratatui terminal: {err}"))?;
        Ok(Self { terminal })
    }
}
