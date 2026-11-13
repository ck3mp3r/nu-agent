//! The browse thread's card fetch: the handed-down client and runtime handle.
//!
//! The browse loop is a bare `std::thread`, so the mechanism under test is the
//! sync/async boundary itself — `Handle::block_on` into a runtime the thread
//! does not own, over a client it did not build. The per-URL state over that
//! fetch lives in `state`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::A2aError;
use crate::client::A2aHttpClient;

use super::{CARD_FETCH_TIMEOUT, fetch_card};

// region:    --- Modules

#[path = "browser/state.rs"]
mod state;

// endregion: --- Modules

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// Support
// ---------------------------------------------------------------------------

/// A client double that records every GET with the deadline it was stamped
/// with, and answers with one canned body.
#[derive(Clone, Default)]
struct RecordingClient {
    calls: Arc<Mutex<Vec<(String, Duration)>>>,
    body: Vec<u8>,
}

impl RecordingClient {
    fn record(&self, url: &str, timeout: Duration) {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((url.to_string(), timeout));
    }

    fn calls(&self) -> Vec<(String, Duration)> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl A2aHttpClient for RecordingClient {
    async fn post_json(
        &self,
        url: &str,
        _body: serde_json::Value,
    ) -> core::result::Result<serde_json::Value, A2aError> {
        Err(A2aError::Internal(format!(
            "RecordingClient: no POST expected for {url}"
        )))
    }

    async fn get_bytes(&self, url: &str) -> core::result::Result<Vec<u8>, A2aError> {
        self.record(url, Duration::ZERO);
        Ok(self.body.clone())
    }

    async fn get_bytes_with_timeout(
        &self,
        url: &str,
        timeout: Duration,
    ) -> core::result::Result<Vec<u8>, A2aError> {
        self.record(url, timeout);
        Ok(self.body.clone())
    }
}

/// The runtime a composition root would hand down.
fn owning_runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The browse thread resolves a card through the handed-down client, entering
/// the owning runtime through the handed-down handle.
///
/// The fetch runs on a bare thread exactly as the browse loop does — no ambient
/// runtime — so a fetch that built its own client and runtime (the drained
/// `CardFetcher`) would neither reach this double nor record a call.
#[test]
fn fetch_card_resolves_through_the_handed_down_client_and_handle() -> Result<()> {
    // -- Setup & Fixtures
    let runtime = owning_runtime()?;
    let card = crate::AgentCard {
        name: "handed-down".to_string(),
        url: "http://127.0.0.1:1".to_string(),
        ..Default::default()
    };
    let client = RecordingClient {
        body: serde_json::to_vec(&card)?,
        ..Default::default()
    };
    let url = "http://127.0.0.1:1/.well-known/agent-card.json";

    // -- Exec
    let fetched = std::thread::spawn({
        let client = client.clone();
        let handle = runtime.handle().clone();
        move || fetch_card(&client, &handle, url)
    })
    .join()
    .map_err(|_| "the browse thread should not panic")?;

    // -- Check
    let fetched = fetched
        .ok()
        .ok_or("the handed-down client should resolve the card")?;
    assert_eq!(fetched.name, "handed-down");
    let calls = client.calls();
    assert_eq!(calls.len(), 1, "exactly one GET: {calls:?}");
    assert_eq!(calls[0].0, url, "the well-known card path");
    assert_eq!(
        calls[0].1, CARD_FETCH_TIMEOUT,
        "the deadline is stamped per request, at the seam the client owns"
    );
    assert_eq!(
        CARD_FETCH_TIMEOUT,
        Duration::from_secs(5),
        "the card-fetch deadline is the drained client's five seconds"
    );
    Ok(())
}

/// A failed fetch is a peer without a card, not a lost peer: the reason is
/// returned for the caller to weigh, and no card is invented.
#[test]
fn fetch_card_returns_the_reason_when_the_transport_fails() -> Result<()> {
    // -- Setup & Fixtures
    let runtime = owning_runtime()?;
    let handle = runtime.handle().clone();
    let url = "http://127.0.0.1:1/.well-known/agent-card.json";

    // -- Exec
    let fetched = std::thread::spawn(move || fetch_card(&NoBodyClient, &handle, url))
        .join()
        .map_err(|_| "the browse thread should not panic")?;

    // -- Check
    let reason = fetched
        .err()
        .ok_or("a failed fetch must not invent a card")?;
    assert!(
        reason.starts_with("connection error:"),
        "the reason names the transport failure: {reason}"
    );
    Ok(())
}
/// A client whose GET fails the way a refused connection does.
#[derive(Clone)]
struct NoBodyClient;

impl A2aHttpClient for NoBodyClient {
    async fn post_json(
        &self,
        url: &str,
        _body: serde_json::Value,
    ) -> core::result::Result<serde_json::Value, A2aError> {
        Err(A2aError::Internal(format!(
            "NoBodyClient: no POST expected for {url}"
        )))
    }

    async fn get_bytes(&self, _url: &str) -> core::result::Result<Vec<u8>, A2aError> {
        Err(A2aError::ConnectionRefused("refused".to_string()))
    }

    async fn get_bytes_with_timeout(
        &self,
        _url: &str,
        _timeout: Duration,
    ) -> core::result::Result<Vec<u8>, A2aError> {
        Err(A2aError::ConnectionRefused("refused".to_string()))
    }
}
