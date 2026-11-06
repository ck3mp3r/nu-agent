//! The loop owner: the `Loop` struct, its named phases, and the channels and
//! state it owns.

// region:    --- Modules

mod owner;
mod sources;

pub use owner::{Loop, run};
pub(crate) use sources::SourceChannels;

// endregion: --- Modules
