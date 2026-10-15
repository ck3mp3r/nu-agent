//! Shared fixtures for the A2A tool handler test modules.
//!
//! Holds the imports and the `Result` alias every topical tool test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use std::sync::Arc;

pub(super) use crate::*;

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Start a loopback A2A server for tool handler tests.
pub(super) async fn test_server() -> A2aServer {
    let card = AgentCard {
        name: "test-agent".into(),
        url: "http://127.0.0.1:0".into(),
        ..Default::default()
    };
    A2aServer::start_with_blocking_timeout(
        card,
        Arc::new(PeerCache::default()),
        0,
        TEST_BLOCKING_TIMEOUT,
    )
    .await
    .unwrap()
}

/// Build a tool context bound to `server`, with a default own card.
pub(super) fn tool_ctx(server: &A2aServer) -> A2aToolContext {
    tool_ctx_with(server, AgentCard::default())
}

/// Build a tool context bound to `server` with an explicit own card.
pub(super) fn tool_ctx_with(server: &A2aServer, own_card: AgentCard) -> A2aToolContext {
    let ctx = A2aToolContext {
        cache: Arc::new(PeerCache::default()),
        client: A2aClient::new().unwrap(),
        own_card,
        task_store: None,
        completion_tx: None,
        runtime_handle: None,
    };
    ctx.cache.add_or_update(Peer {
        name: "test-agent".into(),
        url: server.local_url.clone(),
        host: "127.0.0.1".into(),
        port: server.port,
        card: None,
        discovered_at: std::time::Instant::now(),
    });
    ctx
}
