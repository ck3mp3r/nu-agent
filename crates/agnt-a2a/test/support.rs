//! Crate-wide test support.
//!
//! The production assembly takes its HTTP transport from the composition root,
//! so tests build their own rather than reaching for a crate-level pool.

/// Install the process-wide rustls provider for the test binary.
///
/// The crate itself never installs a provider: the composition root owns that
/// act, and a test that drives the assembly directly is its own root.
pub(crate) fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// A bare test transport: the rustls provider installed, no default headers.
///
/// Callers are tests that own their whole stack — a failure to build here means
/// the process cannot do TLS at all, so the failure is reported by name.
pub(crate) fn test_transport() -> reqwest::Client {
    install_crypto_provider();
    reqwest::Client::builder()
        .build()
        .unwrap_or_else(|e| panic!("test transport should build: {e}"))
}
