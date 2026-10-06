use std::sync::Once;

/// Ensure the rustls crypto provider is installed before creating a reqwest
/// client that uses `rustls-no-provider`.
///
/// The install is process-global and irreversible: the first caller wins and
/// later calls are no-ops. Safe to call multiple times.
pub(crate) fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
