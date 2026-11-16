//! The mechanical assertions, one module per checked property.

// region:    --- Modules

mod composition;
mod construction;
mod enums;
mod filenames;
mod graph;
mod identity;
mod naming;
mod purity;
mod signatures;
mod support;
mod surface;
mod test_doubles;
mod type_names;

pub use composition::*;
pub use construction::*;
pub use enums::*;
pub use filenames::*;
pub use graph::*;
pub use identity::*;
pub use naming::*;
pub use purity::*;
pub use signatures::*;
pub use surface::*;
pub use test_doubles::*;
pub use type_names::*;

// endregion: --- Modules
