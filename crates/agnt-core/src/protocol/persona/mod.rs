//! Persona parsing and resolution — re-exported from `agnt_runtime::protocol`
//! while the runtime layer is strangled out of this crate.

pub use agnt_runtime::protocol::persona::*;

#[cfg(test)]
#[path = "../../../test/protocol/persona/test.rs"]
mod test;

#[cfg(test)]
#[path = "../../../test/protocol/persona/builtins/test.rs"]
mod builtins_test;

#[cfg(test)]
#[path = "../../../test/protocol/persona/error.rs"]
mod error_test;
