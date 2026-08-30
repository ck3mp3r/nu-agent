pub mod factory;
mod info;
pub mod journal;
pub mod prefix;
pub(crate) mod repair;
pub mod resolver;
mod session_impl;
pub mod sqlite_store;
mod store;

#[cfg(test)]
#[path = "prefix_test.rs"]
mod prefix_test;

#[cfg(test)]
#[path = "factory_test.rs"]
mod factory_test;

#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;

#[cfg(test)]
#[path = "journal_test.rs"]
mod journal_test;

#[cfg(test)]
#[path = "repair_test.rs"]
mod repair_test;

#[cfg(test)]
#[path = "resolver_test.rs"]
mod resolver_test;

pub use factory::{SessionStoreBackend, StoreError, StoreType, create_store};
pub use info::SessionInfo;
pub use journal::CachedMemory;
pub use session_impl::{Session, SessionMetadata, extract_title};
pub use store::{CompactionMarker, FsSessionStore, SessionStore, StoreEntry, extract_llm_context};

#[cfg(test)]
mod tool_session_test;
