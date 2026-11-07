//! The mechanical assertions, one module per checked property.

// region:    --- Modules

mod composition;
mod construction;
mod enums;
mod graph;
mod identity;
mod naming;
mod signatures;
mod support;

pub use composition::*;
pub use construction::*;
pub use enums::*;
pub use graph::*;
pub use identity::*;
pub use naming::*;
pub use signatures::*;

// endregion: --- Modules
