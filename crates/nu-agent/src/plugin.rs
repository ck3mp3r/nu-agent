use nu_plugin::{Plugin, PluginCommand};

use crate::command::agent::Agent;
use crate::command::auth::{
    AgentAuthLogin, AgentAuthMcpLogin, AgentAuthMcpLogout, AgentAuthMcpStatus,
};
use crate::command::session::{AgentSessionClear, AgentSessionInspect, AgentSessionList};
use nu_agent_core::session::{SessionStoreImpl, StoreError, StoreType, create_store};

pub struct AgentPlugin {
    pub(crate) runtime: tokio::runtime::Runtime,
    store_type: StoreType,
}

impl AgentPlugin {
    /// Creates a new AgentPlugin. The session store is NOT created at construction
    /// time — it is created lazily on first use via `create_store()`. This avoids
    /// panics during `plugin add` when the cache directory may not exist.
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for AgentPlugin");
        Self {
            runtime,
            store_type: StoreType::Sqlite,
        }
    }

    /// Creates the session store on first use. Called by each command's `run()`
    /// method to obtain the store lazily rather than at plugin construction.
    pub fn create_store(&self) -> Result<SessionStoreImpl, StoreError> {
        self.runtime.block_on(create_store(self.store_type))
    }

    /// Creates the session store with a specific store type.
    /// Used when the CLI `--store` flag overrides the default.
    pub fn create_store_with(&self, store_type: StoreType) -> Result<SessionStoreImpl, StoreError> {
        self.runtime.block_on(create_store(store_type))
    }

    /// Resolve store type from CLI `--store` flag, falling back to plugin default.
    pub fn resolve_store_type(
        &self,
        call: &nu_plugin::EvaluatedCall,
    ) -> Result<StoreType, nu_protocol::LabeledError> {
        if let Some(store_str) = call.get_flag::<String>("store")? {
            store_str.parse().map_err(|e: String| {
                nu_protocol::LabeledError::new(format!("Invalid --store value: {e}"))
                    .with_label("expected 'sqlite' or 'jsonl'", call.head)
            })
        } else {
            Ok(self.store_type)
        }
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
            Box::new(Agent::new()),
            Box::new(AgentAuthLogin::new()),
            Box::new(AgentAuthMcpLogin::new()),
            Box::new(AgentAuthMcpLogout::new()),
            Box::new(AgentAuthMcpStatus::new()),
            Box::new(AgentSessionClear::new()),
            Box::new(AgentSessionInspect::new()),
            Box::new(AgentSessionList::new()),
        ]
    }
}
