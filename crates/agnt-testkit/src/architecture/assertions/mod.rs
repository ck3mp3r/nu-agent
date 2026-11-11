//! The mechanical assertions, one module per checked property.

// region:    --- Modules

mod composition;
mod construction;
mod enums;
mod graph;
mod identity;
mod naming;
mod purity;
mod signatures;
mod support;
mod surface;
mod test_doubles;

pub use composition::*;
pub use construction::*;
pub use enums::*;
pub use graph::*;
pub use identity::*;
pub use naming::*;
pub use purity::*;
pub use signatures::*;
pub use surface::*;
pub use test_doubles::*;

// endregion: --- Modules
