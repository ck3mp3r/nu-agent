pub mod browser;
pub mod filter;
pub mod mdns_discovery;
pub mod service;
pub mod static_discovery;

mod backends;

pub use backends::PeerDiscovery;
pub use browser::*;
pub use service::*;

#[cfg(test)]
#[path = "../../test/discovery/filter.rs"]
mod filter_test;

#[cfg(test)]
#[path = "../../test/discovery/logic.rs"]
mod logic_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/discovery/test.rs"]
mod test;

#[cfg(test)]
#[path = "../../test/discovery/backends.rs"]
mod backends_test;
