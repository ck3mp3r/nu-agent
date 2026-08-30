pub use self::io::{TtyTerminalEvents, open_tty_reader};

mod events;
mod io;

pub use events::*;

#[cfg(test)]
pub(crate) mod events_test;

#[cfg(test)]
mod hybrid_events_test;
