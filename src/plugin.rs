use nu_plugin::{Plugin, PluginCommand};

use crate::commands::agent::Agent;

pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![Box::new(Agent)]
    }
}
