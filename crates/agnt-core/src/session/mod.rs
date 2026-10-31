pub mod factory;
mod info;
pub mod journal;
mod memory_error;
mod model;
pub mod prefix;
pub(crate) mod repair;
pub mod resolver;
mod rig_dir_store;
mod rig_sqlite_store;
pub mod rig_state;
pub mod tool_status;

#[cfg(test)]
#[path = "../../test/session/prefix.rs"]
mod prefix_test;

#[cfg(test)]
#[path = "../../test/session/factory.rs"]
mod factory_test;

#[cfg(test)]
#[path = "../../test/session/journal.rs"]
mod journal_test;

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
#[path = "../../test/session/rig_dir_store.rs"]
mod rig_dir_store_test;

#[cfg(test)]
#[path = "../../test/session/rig_factory.rs"]
mod rig_factory_test;

#[cfg(test)]
#[path = "../../test/session/tool_status.rs"]
mod tool_status_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/session/rig_sqlite_store.rs"]
mod rig_sqlite_store_test;

pub use factory::{
    RigStoreConfig, RigStoreError, StoreType, create_rig_store, create_rig_store_at,
    create_rig_store_in, list_rig_sessions, list_rig_sessions_at, list_rig_sessions_in,
};
pub use info::SessionInfo;
pub use journal::CachedMemory;
pub use memory_error::{MemoryBackendError, MemoryError};
pub use model::Session;
pub use rig_dir_store::{
    NAME_FILE, read_session_name, session_dir, sessions_root, write_session_name,
};
pub use rig_memory::SessionMeta;
pub use rig_sqlite_store::RigSqliteStore;

#[cfg(test)]
#[path = "../../test/session/tool_session.rs"]
mod tool_session_test;
