//! The transport requirement on `AgentBuilder::build`.
//!
//! The transport is a composition-root property: the assembly never builds one,
//! and a missing transport must fail before any listener or mDNS registration
//! starts.

use crate::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_build_without_transport_fails_before_starting_a_listener()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Setup & Fixtures
    // The TLS provider is normally installed by the composition root; a test
    // that drives the assembly directly installs it itself.
    crate::test_support::install_crypto_provider();
    // Reserve a port, then release it: the builder is pointed at it so the
    // assertion below can prove nothing bound it.
    let probe = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = probe.local_addr()?.port();
    drop(probe);

    // -- Exec
    let (error, server) = AgentBuilder::new("no-transport")
        .discovery(PeerDiscoveryImpl::Noop)
        .port(port)
        .blocking_timeout(TEST_BLOCKING_TIMEOUT)
        .build()
        .await
        .err()
        .ok_or("build without a transport must fail")?;

    // -- Check
    assert!(
        server.is_none(),
        "a missing transport must not half-build a server"
    );
    assert!(
        matches!(error, A2aError::Internal(_)),
        "a missing transport must report an internal error, got: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("with_transport"),
        "the error must name the requirement; got: {message}"
    );
    tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .map_err(|e| format!("port {port} must still be unbound: {e}"))?;
    Ok(())
}
