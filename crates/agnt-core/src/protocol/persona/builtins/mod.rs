pub mod content;

pub use content::*;

#[cfg(test)]
#[path = "../../../../test/protocol/persona/builtins/test.rs"]
mod test;
