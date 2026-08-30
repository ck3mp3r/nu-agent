pub mod classify;
pub mod defaults;
pub mod model_family;
pub mod resolve;

pub use classify::classify_model_family;
pub use defaults::PreambleDefaults;
pub use model_family::*;
pub use resolve::{UserPreambleInput, resolve_preamble};

#[cfg(test)]
mod test;
