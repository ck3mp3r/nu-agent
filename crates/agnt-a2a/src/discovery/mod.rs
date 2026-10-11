pub mod browser;
pub mod card;
pub mod filter;
pub mod mdns_discovery;
pub mod service;
pub mod static_discovery;

mod impl_enum;

pub use browser::*;
pub use impl_enum::PeerDiscoveryImpl;
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
#[path = "../../test/discovery/mdns_discovery.rs"]
mod mdns_discovery_test;

#[cfg(test)]
#[path = "../../test/discovery/impl_enum.rs"]
mod impl_enum_test;
