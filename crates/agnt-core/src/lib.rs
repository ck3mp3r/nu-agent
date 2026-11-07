pub mod bus;
pub mod conversation;
pub mod hook;
pub mod hooks;
pub mod orchestrator;
pub mod protocol;
pub mod renderer;
pub mod session;
pub mod types;

#[cfg(test)]
#[path = "../test/architecture/test.rs"]
mod architecture_test;

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;
