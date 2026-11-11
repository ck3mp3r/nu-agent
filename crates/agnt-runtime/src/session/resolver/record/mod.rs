//! The transcript record's two directions in the resolver: the fold that writes
//! a session's blocks as the store's rows, and the read that builds the entries
//! the hydration consumes from those rows.

// region:    --- Modules

mod fold;
mod read;
mod store;

pub(super) use fold::result_text;
pub(crate) use read::read_entries;

// endregion: --- Modules
