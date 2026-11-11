//! Test support for the discovery integration suites.
//!
//! Only the integration discovery tests build a card client, so the helper
//! lives beside them: with default features no caller compiles, and a module
//! that follows its callers leaves no dead helper behind.

/// An A2A client over a fresh test transport.
///
/// The discovery seams take the client as a handed-down fact; a test that owns
/// its stack builds one here rather than reaching into the crate.
pub(crate) fn test_card_client() -> crate::A2aClient {
    crate::A2aClient::new(crate::test_support::test_transport())
}
