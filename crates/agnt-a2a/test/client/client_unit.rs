use super::A2aClient;

// ---------------------------------------------------------------------------
// A2aClient construction
// ---------------------------------------------------------------------------

#[test]
fn test_default_client() {
    let client = A2aClient::new(crate::test_support::test_transport());
    // Just verify it doesn't panic
    let _ = client;
}
