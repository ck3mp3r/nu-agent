pub mod safety;
pub mod terminal;
pub mod transport;

#[cfg(test)]
#[path = "../../test/platform/safety.rs"]
mod safety_test;

#[cfg(test)]
#[path = "../../test/platform/terminal.rs"]
mod terminal_test;

#[cfg(test)]
#[path = "../../test/platform/transport.rs"]
mod transport_test;
