use nu_plugin::{JsonSerializer, serve_plugin};
use nu_plugin_agent::AgentPlugin;

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    serve_plugin(&AgentPlugin::new(), JsonSerializer)
}
