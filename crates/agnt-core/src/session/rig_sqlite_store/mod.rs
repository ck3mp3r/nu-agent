//! A rig-memory [`Store`](rig_memory::Store) over SQLite.
//!
//! One database holds every session: the `logs` and `files` tables are opaque
//! BLOB stores keyed by session id and name. [`Store`](rig_memory::Store) is
//! synchronous while sqlx is asynchronous, so every call runs on a dedicated
//! worker thread that owns the connection pool and its runtime.

// region:    --- Modules

mod actor;
mod open;
mod traits;

pub use actor::RigSqliteStore;

// endregion: --- Modules
