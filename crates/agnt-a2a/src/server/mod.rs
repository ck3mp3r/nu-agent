mod a2a_server;

pub mod handlers;
mod middleware;
mod response;

#[cfg(all(test, feature = "integration"))]
#[path = "../../test/server/test.rs"]
mod test;

pub use a2a_server::{
    A2aServer, AppState, DEFAULT_BLOCKING_TIMEOUT, MAX_TEST_BLOCKING_TIMEOUT, TEST_BLOCKING_TIMEOUT,
};
