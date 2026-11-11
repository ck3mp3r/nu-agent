//! Session assembly and driving.
//!
//! The store's schema and services stay in `agnt-storage`; this module builds
//! the store a session writes through and drives the writes.

// region:    --- Modules

pub mod blocking;
pub mod factory;
mod info;
pub mod journal;
pub mod management;
mod memory_error;
mod model;
pub mod prefix;
pub mod repair;
pub mod resolver;
pub mod rig_state;
pub mod settle;
pub mod tool_status;

#[cfg(test)]
#[path = "../../test/session/blocking.rs"]
mod blocking_test;

#[cfg(test)]
#[path = "../../test/session/prefix.rs"]
mod prefix_test;

#[cfg(test)]
#[path = "../../test/session/settle.rs"]
mod settle_test;

#[cfg(test)]
#[path = "../../test/session/factory.rs"]
mod factory_test;

#[cfg(test)]
#[path = "../../test/session/journal.rs"]
mod journal_test;

#[cfg(test)]
#[path = "../../test/session/management.rs"]
mod management_test;

#[cfg(test)]
#[path = "../../test/session/memory_error.rs"]
mod memory_error_test;

#[cfg(test)]
#[path = "../../test/session/repair.rs"]
mod repair_test;

#[cfg(test)]
#[path = "../../test/session/resolver.rs"]
mod resolver_test;

#[cfg(test)]
#[path = "../../test/session/rig_state.rs"]
mod rig_state_test;

#[cfg(test)]
#[path = "../../test/session/rig_factory.rs"]
mod rig_factory_test;

#[cfg(test)]
#[path = "../../test/session/tool_status.rs"]
mod tool_status_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/session/rig_blobs.rs"]
mod rig_blobs_test;

// endregion: --- Modules

// region:    --- Reexports

pub use crate::config::StoreType;
pub use factory::{
    RigStoreConfig, RigStoreError, create_rig_store, create_rig_store_at, create_rig_store_in,
    list_rig_sessions, list_rig_sessions_at, list_rig_sessions_in,
};
pub use info::SessionInfo;
pub use journal::CachedMemory;
pub use memory_error::{MemoryBackendError, MemoryError};
pub use model::Session;
pub use rig_memory::SessionMeta;

// endregion: --- Reexports

#[cfg(test)]
#[path = "../../test/session/tool_session.rs"]
mod tool_session_test;
