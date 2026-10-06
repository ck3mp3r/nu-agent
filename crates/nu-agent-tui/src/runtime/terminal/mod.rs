pub use self::io::{TtyTerminalEvents, open_tty_reader};

mod events;
mod io;

pub use events::*;

#[cfg(test)]
#[path = "../../../test/runtime/terminal/events.rs"]
pub(crate) mod events_test;

#[cfg(test)]
#[path = "../../../test/runtime/terminal/hybrid_events.rs"]
mod hybrid_events_test;
