use nu_plugin::{JsonSerializer, serve_plugin};
use nu_plugin_agent::AgentPlugin;

fn main() {
    // install_default returns Err(Arc<CryptoProvider>) only when a provider is
    // already installed (e.g. by an embedded runtime) — that is not fatal. A
    // genuine install failure is impossible because the Err variant carries the
    // already-installed provider, so we simply continue in all cases.
    let _ = rustls::crypto::ring::default_provider().install_default();
    serve_plugin(&AgentPlugin::default(), JsonSerializer)
}
