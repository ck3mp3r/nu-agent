pub mod factory;
mod info;
pub mod journal;
mod model;
pub mod prefix;
pub(crate) mod repair;
pub mod resolver;
pub mod sqlite_store;
mod store;

#[cfg(test)]
#[path = "../../test/session/prefix.rs"]
mod prefix_test;

#[cfg(test)]
#[path = "../../test/session/factory.rs"]
mod factory_test;

#[cfg(test)]
#[path = "../../test/session/store.rs"]
mod store_test;

#[cfg(test)]
#[path = "../../test/session/journal.rs"]
mod journal_test;

#[cfg(test)]
#[path = "../../test/session/repair.rs"]
mod repair_test;

#[cfg(test)]
#[path = "../../test/session/resolver.rs"]
mod resolver_test;

pub use factory::{SessionStoreBackend, StoreError, StoreType, create_store};
pub use info::SessionInfo;
pub use journal::CachedMemory;
pub use model::{Session, SessionMetadata, extract_title};
pub use store::{CompactionMarker, FsSessionStore, SessionStore, StoreEntry, extract_llm_context};

#[cfg(test)]
#[path = "../../test/session/tool_session.rs"]
mod tool_session_test;
