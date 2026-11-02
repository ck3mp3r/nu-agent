pub mod safety;
pub mod terminal;

#[cfg(test)]
#[path = "../../test/platform/safety.rs"]
mod safety_test;

#[cfg(test)]
#[path = "../../test/platform/terminal.rs"]
mod terminal_test;
