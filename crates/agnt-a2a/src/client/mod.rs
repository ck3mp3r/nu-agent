mod a2a_client;
mod functions;
mod http_client;
#[cfg(test)]
#[path = "../../test/client/mock_support.rs"]
mod mock;

pub use a2a_client::*;
pub use functions::*;
pub use http_client::*;
#[cfg(test)]
pub use mock::*;

#[cfg(test)]
#[path = "../../test/client/client_unit.rs"]
mod client_unit_test;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/client/a2a_client.rs"]
mod a2a_client_test;
#[cfg(all(test, feature = "integration"))]
#[path = "../../test/client/functions.rs"]
mod functions_test;
#[cfg(test)]
#[path = "../../test/client/mock.rs"]
mod mock_test;
