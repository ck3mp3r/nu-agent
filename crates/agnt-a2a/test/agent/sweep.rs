use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::{CardFetcher, SWEEP_MAX_FAILURES, sweep_peers};
use crate::{A2aError, AgentCard, Peer, PeerCache};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
type FetchOutcome = core::result::Result<AgentCard, A2aError>;

// region:    --- Test Support

/// Mock card fetcher: pops one canned outcome per call and records the URL.
struct MockFetcher {
    outcomes: Mutex<VecDeque<FetchOutcome>>,
    requested: Mutex<Vec<String>>,
}

impl MockFetcher {
    fn new(outcomes: Vec<FetchOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            requested: Mutex::new(Vec::new()),
        }
    }

    fn requested_urls(&self) -> Vec<String> {
        lock(&self.requested).clone()
    }
}

impl CardFetcher for MockFetcher {
    fn fetch_card(&self, peer_url: &str) -> impl std::future::Future<Output = FetchOutcome> + Send {
        lock(&self.requested).push(peer_url.to_string());
        let outcome = lock(&self.outcomes)
            .pop_front()
            .unwrap_or_else(|| Err(A2aError::Internal("mock fetcher exhausted".to_string())));
        async move { outcome }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn peer_fixture(name: &str, port: u16) -> Peer {
    Peer {
        name: name.to_string(),
        url: format!("http://127.0.0.1:{port}"),
        host: "127.0.0.1".to_string(),
        port,
        card: None,
        discovered_at: Instant::now(),
    }
}

/// A card as the peer serves it: the bare agent name, no port suffix.
fn card_fixture(name: &str) -> AgentCard {
    AgentCard {
        name: name.to_string(),
        url: "http://127.0.0.1:0".to_string(),
        version: "2.0".to_string(),
        ..Default::default()
    }
}

fn refused() -> FetchOutcome {
    Err(A2aError::ConnectionRefused("mock refused".to_string()))
}

// endregion: --- Test Support

// region:    --- Tests

#[tokio::test]
async fn test_sweep_pass_refresh_updates_entry_in_place() -> Result<()> {
    // -- Setup & Fixtures
    let cache = PeerCache::default();
    let own = peer_fixture("agent-40000", 40000);
    cache.add_or_update(own.clone());
    cache.add_or_update(peer_fixture("researcher-51234", 51234));
    cache.add_or_update(peer_fixture("planner-51235", 51235));
    cache.add_or_update(peer_fixture("writer-51236", 51236));
    let fetcher = MockFetcher::new(vec![
        Ok(card_fixture("remote-agent")),
        Ok(card_fixture("remote-agent")),
        Ok(card_fixture("remote-agent")),
    ]);
    let mut failures = HashMap::new();

    // -- Exec
    let started = Instant::now();
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;
    let elapsed = started.elapsed();

    // -- Check
    assert!(
        elapsed < Duration::from_secs(1),
        "a 3-peer pass must stay under 1s, took {elapsed:?}"
    );
    let refreshed = cache
        .get("researcher-51234")
        .ok_or("peer should stay under its mDNS instance key")?;
    let card = refreshed
        .card
        .ok_or("refreshed peer should carry the fetched card")?;
    assert_eq!(card.name, "remote-agent");
    assert_eq!(refreshed.url, "http://127.0.0.1:51234");
    assert!(
        cache.get("remote-agent").is_none(),
        "entry must not be re-keyed by the fetched card name"
    );
    assert_eq!(cache.list().len(), 4);
    assert_eq!(failures.get("researcher-51234"), Some(&0));
    Ok(())
}

#[tokio::test]
async fn test_sweep_pass_evicts_after_max_consecutive_failures() -> Result<()> {
    // -- Setup & Fixtures
    let cache = PeerCache::default();
    let own = peer_fixture("agent-40000", 40000);
    cache.add_or_update(own.clone());
    cache.add_or_update(peer_fixture("ghost-51999", 51999));
    let fetcher = MockFetcher::new(vec![refused(), refused(), refused()]);
    let mut failures = HashMap::new();

    // -- Exec
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;
    let after_first = cache.get("ghost-51999").is_some();
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;
    let after_second = cache.get("ghost-51999").is_some();
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;
    let after_third = cache.get("ghost-51999").is_some();

    // -- Check
    assert!(after_first, "one failure must not evict");
    assert!(
        after_second,
        "two failures must not evict — eviction happens on failure {SWEEP_MAX_FAILURES}"
    );
    assert!(!after_third, "peer must be evicted after 3 failures");
    assert!(cache.get("agent-40000").is_some(), "own entry must survive");
    Ok(())
}

#[tokio::test]
async fn test_sweep_pass_skips_own_url() -> Result<()> {
    // -- Setup & Fixtures
    let cache = PeerCache::default();
    let own = peer_fixture("agent-40000", 40000);
    cache.add_or_update(own.clone());
    cache.add_or_update(peer_fixture("researcher-51234", 51234));
    let fetcher = MockFetcher::new(vec![Ok(card_fixture("researcher"))]);
    let mut failures = HashMap::new();

    // -- Exec
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;

    // -- Check
    assert_eq!(
        fetcher.requested_urls(),
        vec!["http://127.0.0.1:51234".to_string()],
        "only the remote peer's card is fetched"
    );
    let own_entry = cache
        .get("agent-40000")
        .ok_or("own peer entry should exist")?;
    assert!(
        own_entry.card.is_none(),
        "own entry must not be refreshed by the sweep"
    );
    Ok(())
}

#[tokio::test]
async fn test_sweep_pass_empty_cache_makes_no_fetch() -> Result<()> {
    // -- Setup & Fixtures
    let cache = PeerCache::default();
    let own = peer_fixture("agent-40000", 40000);
    cache.add_or_update(own.clone());
    let fetcher = MockFetcher::new(vec![]);
    let mut failures = HashMap::new();

    // -- Exec
    sweep_peers(&cache, &own.url, &fetcher, &mut failures).await;

    // -- Check
    assert!(fetcher.requested_urls().is_empty());
    assert!(failures.is_empty());
    Ok(())
}

// endregion: --- Tests
