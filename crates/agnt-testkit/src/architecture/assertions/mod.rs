//! The mechanical assertions, one module per checked property.

// region:    --- Modules

mod composition;
mod enums;
mod graph;
mod identity;
mod signatures;
mod support;

pub use composition::*;
pub use enums::*;
pub use graph::*;
pub use identity::*;
pub use signatures::*;

// endregion: --- Modules
