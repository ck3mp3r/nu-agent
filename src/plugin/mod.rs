use nu_plugin::{Plugin, PluginCommand};

use crate::commands::agent::Agent;
use crate::commands::agent::session::{AgentSessionClear, AgentSessionInspect, AgentSessionList};
use crate::llm::runtime::LlmRuntime;
use crate::session::SessionStore;
use std::sync::Arc;

pub struct AgentPlugin {
    session_store: SessionStore,
    llm_runtime: Arc<LlmRuntime>,
}

#[derive(Clone)]
pub struct RuntimeCtx {
    llm_runtime: Arc<LlmRuntime>,
}

impl RuntimeCtx {
    pub fn new(llm_runtime: Arc<LlmRuntime>) -> Self {
        Self { llm_runtime }
    }

    pub fn llm_runtime(&self) -> &LlmRuntime {
        self.llm_runtime.as_ref()
    }
}

impl AgentPlugin {
    /// Creates a new AgentPlugin with default SessionStore
    pub fn new() -> Self {
        Self {
            session_store: SessionStore::new(),
            llm_runtime: Arc::new(LlmRuntime::new()),
        }
    }

    /// Creates a new AgentPlugin with a custom SessionStore (for testing)
    #[cfg(test)]
    pub fn new_with_store(session_store: SessionStore, llm_runtime: Arc<LlmRuntime>) -> Self {
        Self {
            session_store,
            llm_runtime,
        }
    }

    #[cfg(test)]
    pub fn llm_runtime(&self) -> Arc<LlmRuntime> {
        self.llm_runtime.clone()
    }

    pub fn runtime_ctx(&self) -> RuntimeCtx {
        RuntimeCtx::new(self.llm_runtime.clone())
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
            Box::new(Agent::new(self.session_store.clone(), self.runtime_ctx())),
            Box::new(AgentSessionClear::new(self.session_store.clone())),
            Box::new(AgentSessionInspect::new(self.session_store.clone())),
            Box::new(AgentSessionList::new(self.session_store.clone())),
        ]
    }
}

#[cfg(test)]
mod tests;
