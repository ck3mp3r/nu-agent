//! The loop owner: the `Loop` struct, its named phases, and the channels and
//! state it owns.

// region:    --- Modules

mod external_queue;
mod owner;
mod pending_completions;
mod sources;
mod stages;

pub(crate) use external_queue::{CancelDisposition, ExternalQueue};
pub use owner::{Loop, run};
pub(crate) use pending_completions::CompletionBuffer;
pub(crate) use sources::SourceChannels;
pub(crate) use stages::ProductionStages;

// endregion: --- Modules
