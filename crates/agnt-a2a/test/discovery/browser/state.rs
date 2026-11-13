//! The browse thread's per-URL fetch and announcement state.
//!
//! `resolve_peer` is the whole decision one mDNS resolution makes: fetch the
//! card or leave it, announce the peer or stay quiet, and what the log says
//! about either. These tests drive it with a clock they control, so a
//! five-minute cooldown costs no time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::A2aError;
use crate::client::A2aHttpClient;

use super::super::{PEER_FETCH_COOLDOWN, resolve_peer};
use super::{RecordingClient, Result, owning_runtime};

// ---------------------------------------------------------------------------
// Support
// ---------------------------------------------------------------------------

/// Every record captured on this thread since the test set up, cleared as it is
/// read.
///
/// `testing_logger` captures into a thread-local buffer, so two tests never see
/// each other's records: under `cargo test`, which runs the whole suite in one
/// process on a shared thread pool, and under nextest's process-per-test alike.
fn captured() -> Vec<(log::Level, String)> {
    let found = std::cell::RefCell::new(Vec::new());
    testing_logger::validate(|logs| {
        *found.borrow_mut() = logs
            .iter()
            .map(|log| (log.level, log.body.clone()))
            .collect();
    });
    found.into_inner()
}

/// The records of `logs` whose message contains `fragment`.
fn matching(logs: &[(log::Level, String)], fragment: &str) -> Vec<(log::Level, String)> {
    logs.iter()
        .filter(|(_, body)| body.contains(fragment))
        .cloned()
        .collect()
}

/// The peer a resolution produces before its card is fetched.
fn peer_named(name: &str, url: &str) -> crate::peer::Peer {
    crate::peer::Peer {
        name: name.to_string(),
        url: url.to_string(),
        host: "127.0.0.1".to_string(),
        port: 41001,
        card: None,
        discovered_at: std::time::Instant::now(),
    }
}

/// A client that records every GET and refuses it, the way a dead port does.
#[derive(Clone, Default)]
struct RefusingClient {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RefusingClient {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl A2aHttpClient for RefusingClient {
    async fn post_json(
        &self,
        url: &str,
        _body: serde_json::Value,
    ) -> core::result::Result<serde_json::Value, A2aError> {
        Err(A2aError::Internal(format!(
            "RefusingClient: no POST expected for {url}"
        )))
    }

    async fn get_bytes(&self, _url: &str) -> core::result::Result<Vec<u8>, A2aError> {
        Err(A2aError::ConnectionRefused("refused".to_string()))
    }

    async fn get_bytes_with_timeout(
        &self,
        url: &str,
        _timeout: Duration,
    ) -> core::result::Result<Vec<u8>, A2aError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(url.to_string());
        Err(A2aError::ConnectionRefused("refused".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// WHEN a live peer re-resolves, THEN it SHALL publish once and stop re-fetching
/// its held card, until `PeerLost` forgets the URL.
#[test]
fn resolve_peer_publishes_once_per_url_until_the_peer_is_lost() -> Result<()> {
    // -- Setup & Fixtures
    let runtime = owning_runtime()?;
    let handle = runtime.handle().clone();
    let card = crate::AgentCard {
        name: "once".to_string(),
        url: "http://127.0.0.1:41001".to_string(),
        ..Default::default()
    };
    let client = RecordingClient {
        body: serde_json::to_vec(&card)?,
        ..Default::default()
    };
    let mut states = HashMap::new();
    let now = std::time::Instant::now();
    let peer = || peer_named("once-peer", "http://127.0.0.1:41001");

    // -- Exec
    let first = resolve_peer(&mut states, peer(), &client, &handle, now);
    let second = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(1),
    );
    let third = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(2),
    );

    // -- Check: the first resolution is news, the repeats are not.
    let first = first.ok_or("the first resolution publishes the peer")?;
    assert_eq!(
        first.card.map(|card| card.name),
        Some("once".to_string()),
        "the published peer carries the card it resolved"
    );
    assert!(
        second.is_none() && third.is_none(),
        "a re-resolution of an announced URL publishes nothing"
    );
    assert_eq!(
        client.calls().len(),
        1,
        "a held card is not fetched again: {:?}",
        client.calls()
    );

    // -- Exec: PeerLost forgets the URL's state.
    states.retain(|_, state| state.name != "once-peer");
    let after_lost = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(3),
    );

    // -- Check
    assert!(
        after_lost.is_some(),
        "a peer that comes back announces itself again"
    );
    Ok(())
}

/// WHEN one URL keeps failing its card fetch, THEN the browse thread SHALL warn
/// twice, demote every later failure to debug, and cool the URL down.
#[test]
fn resolve_peer_warns_twice_then_cools_the_url_down() -> Result<()> {
    // -- Setup & Fixtures
    testing_logger::setup();
    let runtime = owning_runtime()?;
    let handle = runtime.handle().clone();
    let client = RefusingClient::default();
    let mut states = HashMap::new();
    let now = std::time::Instant::now();
    let peer = || peer_named("dead-peer", "http://127.0.0.1:41002");

    // -- Exec: five resolutions — two warnings, then a cooled-down retry.
    let first = resolve_peer(&mut states, peer(), &client, &handle, now);
    let second = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(1),
    );
    let third = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(2),
    );
    let in_cooldown = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + Duration::from_secs(3),
    );
    let after_cooldown = resolve_peer(
        &mut states,
        peer(),
        &client,
        &handle,
        now + PEER_FETCH_COOLDOWN + Duration::from_secs(4),
    );

    // -- Check: the peer names itself once, and no repeat publishes.
    assert!(
        first.is_some(),
        "a peer without a card is still a peer: it is announced once"
    );
    assert!(
        [second, third, in_cooldown, after_cooldown]
            .iter()
            .all(Option::is_none),
        "no repeat resolution publishes"
    );

    // -- Check: two warnings per failure epoch, then debug.
    let logs = captured();
    let warns = matching(&logs, "card fetch failed for");
    assert_eq!(warns.len(), 2, "two warnings, no more: {warns:?}");
    assert!(
        warns.iter().all(|(level, _)| *level == log::Level::Warn),
        "a failure inside the budget is a warning: {warns:?}"
    );
    let repeats = matching(&logs, "card fetch failed again");
    assert_eq!(
        repeats.len(),
        2,
        "every later failure is a repeat: {repeats:?}"
    );
    assert!(
        repeats.iter().all(|(level, _)| *level == log::Level::Debug),
        "a repeat is demoted to debug: {repeats:?}"
    );
    let skipped = matching(&logs, "card fetch skipped for");
    assert_eq!(
        skipped.len(),
        1,
        "the cooldown skips exactly the resolutions inside it: {skipped:?}"
    );
    assert_eq!(
        client.calls().len(),
        4,
        "one attempt per resolution, minus the one the cooldown skipped: {:?}",
        client.calls()
    );
    Ok(())
}

/// WHEN a URL's card fetch succeeds again, THEN its failure state SHALL reset
/// and the recovery SHALL be logged once at info.
#[test]
fn resolve_peer_resets_the_state_and_logs_the_recovery_once() -> Result<()> {
    // -- Setup & Fixtures
    testing_logger::setup();
    let runtime = owning_runtime()?;
    let handle = runtime.handle().clone();
    let card = crate::AgentCard {
        name: "recovered".to_string(),
        url: "http://127.0.0.1:41003".to_string(),
        ..Default::default()
    };
    let refusing = RefusingClient::default();
    let recovering = RecordingClient {
        body: serde_json::to_vec(&card)?,
        ..Default::default()
    };
    let mut states = HashMap::new();
    let now = std::time::Instant::now();
    let peer = || peer_named("recovering-peer", "http://127.0.0.1:41003");

    // -- Exec: two failures, then a success.
    let _ = resolve_peer(&mut states, peer(), &refusing, &handle, now);
    let _ = resolve_peer(
        &mut states,
        peer(),
        &refusing,
        &handle,
        now + Duration::from_secs(1),
    );
    let after_recovery = resolve_peer(
        &mut states,
        peer(),
        &recovering,
        &handle,
        now + Duration::from_secs(2),
    );

    // -- Check: the change logs once at info...
    let logs = captured();
    let recovered = matching(&logs, "card fetch recovered for");
    assert_eq!(recovered.len(), 1, "one recovery line: {recovered:?}");
    assert_eq!(
        recovered[0].0,
        log::Level::Info,
        "the recovery is news, not a warning"
    );
    // -- Check: ...the state is clean, and the repeated URL stays quiet.
    let state = states
        .get("http://127.0.0.1:41003")
        .ok_or("the URL's state outlives its failures")?;
    assert_eq!(state.consecutive_failures, 0, "a success resets the count");
    assert!(state.card_held, "the card is in hand now");
    assert!(
        after_recovery.is_none(),
        "the URL was announced already, so the recovery publishes nothing"
    );
    assert_eq!(
        recovering.calls().len(),
        1,
        "the recovered card is fetched once, not per resolution"
    );
    Ok(())
}
