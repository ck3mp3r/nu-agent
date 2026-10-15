//! Shared fixtures for the a2a client test roots.
//!
//! Both `a2a_client.rs` and `functions.rs` declare this module, so the
//! real-server setup and the non-blocking send configuration live in one place.

use std::sync::Arc;

use crate::client::A2aClient;
use crate::{AgentCapabilities, AgentCard, PeerCache, SendMessageConfiguration};

// region:    --- Test Support

/// Non-blocking configuration: these tests assert on the in-progress task, so
/// the server must not wait for a terminal state (spec §3.2.2).
pub(super) fn non_blocking() -> Option<SendMessageConfiguration> {
    Some(SendMessageConfiguration {
        return_immediately: Some(true),
        accepted_output_modes: None,
    })
}

/// Start a test server, return (server, client, server_url).
pub(super) async fn test_setup() -> (crate::A2aServer, A2aClient, String) {
    let card = AgentCard {
        name: "test-server".into(),
        url: "http://127.0.0.1:0".into(),
        version: "1.0".into(),
        capabilities: AgentCapabilities::default(),
        skills: vec![],
        ..Default::default()
    };
    let server = crate::A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        crate::TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap();
    let client = A2aClient::new().unwrap();
    let url = server.local_url.clone();
    (server, client, url)
}

// endregion: --- Test Support
