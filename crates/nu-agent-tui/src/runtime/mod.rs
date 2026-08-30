mod backend;
pub(crate) mod branch_watcher;
pub mod coordinator;
pub mod layout;
mod panels;
pub(crate) mod render;
mod renderer;
mod session_picker;
mod status;
mod terminal;
pub(crate) use backend::LiveTerminalUi;
pub use backend::{
    AnsiTerminalBackend, RuntimeRunError, run_with_terminal_restore, run_with_terminal_restore_sync,
};
pub use coordinator::*;
pub use layout::*;
use panels::*;
pub use renderer::TuiRuntimeRenderer;
use status::help::*;
#[cfg(test)]
pub use terminal::CrosstermTerminalEvents;
#[cfg(test)]
pub use terminal::events_test::ScriptedTerminalEvents;
#[cfg(test)]
pub(crate) use terminal::events_test::map_crossterm_event_for_test;
pub(crate) use terminal::map_crossterm_event;
pub use terminal::{HybridTerminalEvents, InputSourceDiagnostics, TerminalEventSource};
pub use terminal::{TtyTerminalEvents, open_tty_reader};

#[cfg(test)]
pub(super) mod test;

#[cfg(test)]
mod panels_test;

#[cfg(test)]
mod renderer_test;

#[cfg(test)]
mod session_picker_test;

#[cfg(test)]
pub(crate) use status::help_test::*;
