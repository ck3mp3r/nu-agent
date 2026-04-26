use nu_plugin::{Plugin, PluginCommand};

use crate::commands::agent::Agent;
use crate::commands::agent_session_list::AgentSessionList;
use crate::session::SessionStore;

pub struct AgentPlugin {
    session_store: SessionStore,
}

impl AgentPlugin {
    /// Creates a new AgentPlugin with default SessionStore
    pub fn new() -> Self {
        Self {
            session_store: SessionStore::new(),
        }
    }

    /// Creates a new AgentPlugin with a custom SessionStore (for testing)
    #[cfg(test)]
    pub fn new_with_store(session_store: SessionStore) -> Self {
        Self { session_store }
    }
}

impl Default for AgentPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for AgentPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![
            Box::new(Agent),
            Box::new(AgentSessionList::new(self.session_store.clone())),
        ]
    }
}
