use nu_plugin::{Plugin, PluginCommand};

use crate::agent::application::command::Agent;
use crate::agent::session::commands::{AgentSessionClear, AgentSessionInspect, AgentSessionList};
use crate::session::SessionStore;

pub struct AgentPlugin {
    session_store: SessionStore,
}

#[derive(Clone)]
pub struct RuntimeCtx {}

impl RuntimeCtx {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RuntimeCtx {
    fn default() -> Self {
        Self::new()
    }
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

    pub fn runtime_ctx(&self) -> RuntimeCtx {
        RuntimeCtx::new()
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
            Box::new(Agent::new(self.session_store.clone(), RuntimeCtx::new())),
            Box::new(AgentSessionClear::new(self.session_store.clone())),
            Box::new(AgentSessionInspect::new(self.session_store.clone())),
            Box::new(AgentSessionList::new(self.session_store.clone())),
        ]
    }
}

#[cfg(test)]
mod test;
