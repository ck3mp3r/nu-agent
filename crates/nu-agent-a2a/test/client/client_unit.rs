use super::A2aClient;

// ---------------------------------------------------------------------------
// A2aClient construction
// ---------------------------------------------------------------------------

#[test]
fn test_default_client() {
    let client = A2aClient::new().unwrap();
    // Just verify it doesn't panic
    let _ = client;
}

#[test]
fn test_default_trait() {
    let client = A2aClient::default();
    let _ = client;
}
