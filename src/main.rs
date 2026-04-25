use nu_plugin::{JsonSerializer, serve_plugin};
use nu_plugin_agent::AgentPlugin;

fn main() {
    serve_plugin(&AgentPlugin, JsonSerializer)
}
