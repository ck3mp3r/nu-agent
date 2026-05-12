use std::panic::{self, AssertUnwindSafe};

use crate::agent::ui::tui::platform::terminal::{
    TerminalBackend, TerminalLifecycle, TerminalLifecycleError,
};

pub trait TerminalRestorer {
    type Error;

    fn restore_terminal(&mut self) -> Result<(), Self::Error>;
}

impl<B> TerminalRestorer for TerminalLifecycle<B>
where
    B: TerminalBackend,
{
    type Error = TerminalLifecycleError;

    fn restore_terminal(&mut self) -> Result<(), Self::Error> {
        self.restore()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreRunError<E, R> {
    Run(E),
    RunWithRestore { run_error: E, restore_error: R },
    Restore(R),
}

pub fn run_with_restore<T, E, R, F>(
    restorer: &mut impl TerminalRestorer<Error = R>,
    run: F,
) -> Result<T, RestoreRunError<E, R>>
where
    F: FnOnce() -> Result<T, E>,
{
    let run_result = panic::catch_unwind(AssertUnwindSafe(run));

    match run_result {
        Ok(Ok(value)) => {
            restorer
                .restore_terminal()
                .map_err(RestoreRunError::Restore)?;
            Ok(value)
        }
        Ok(Err(run_error)) => match restorer.restore_terminal() {
            Ok(()) => Err(RestoreRunError::Run(run_error)),
            Err(restore_error) => Err(RestoreRunError::RunWithRestore {
                run_error,
                restore_error,
            }),
        },
        Err(panic_payload) => {
            let _ = restorer.restore_terminal();
            panic::resume_unwind(panic_payload)
        }
    }
}
