use std::sync::Once;
use std::time::Duration;

use reqwest::Client;

use crate::AgentCard;

/// Ensure the rustls crypto provider is installed before creating a reqwest
/// [`Client`] that uses `rustls-no-provider`.  Safe to call multiple times.
fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Shared state for best-effort card fetching, populated lazily inside the
/// browse thread (a `std::thread`, not a tokio context) so that dropping the
/// inner `tokio::runtime::Runtime` never triggers the "Cannot drop a runtime
/// in a blocking context" panic.
pub(crate) struct CardFetcher {
    client: Option<Client>,
    rt: Option<tokio::runtime::Runtime>,
}

impl CardFetcher {
    pub(crate) fn new() -> Self {
        ensure_crypto_provider();
        let client = match Client::builder().timeout(Duration::from_secs(5)).build() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("CardFetcher: failed to create HTTP client: {e}");
                None
            }
        };
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("CardFetcher: failed to create tokio runtime: {e}");
                None
            }
        };
        Self { client, rt }
    }

    /// Best-effort fetch of an [`AgentCard`] from the given URL.
    /// Returns `None` on any error (connection refused, timeout, non-200,
    /// bad JSON).
    ///
    /// Called from a plain OS thread (std::thread::spawn) with no
    /// existing tokio runtime, so block_on at this boundary is correct.
    pub(crate) fn fetch_card(&self, url: &str) -> Option<AgentCard> {
        let (client, rt) = (self.client.as_ref()?, self.rt.as_ref()?);
        rt.block_on(async {
            let resp = match client.get(url).send().await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("card fetch failed (connection error) for {url}: {e}");
                    return None;
                }
            };
            if !resp.status().is_success() {
                log::warn!(
                    "card fetch failed (HTTP {}) for {url}",
                    resp.status().as_u16()
                );
                return None;
            }
            match resp.json::<AgentCard>().await {
                Ok(card) => Some(card),
                Err(e) => {
                    log::warn!("card fetch failed (bad JSON) for {url}: {e}");
                    None
                }
            }
        })
    }
}

pub(crate) static CARD_FETCHER: std::sync::OnceLock<CardFetcher> = std::sync::OnceLock::new();
