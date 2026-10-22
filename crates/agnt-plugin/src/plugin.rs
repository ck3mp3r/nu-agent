use nu_plugin::{Plugin, PluginCommand};

use crate::command::agent::Agent;
use crate::command::config::AgentConfigInit;
use crate::command::mcp::{AgentAuthMcpLogin, AgentAuthMcpLogout, AgentAuthMcpStatus};
use crate::command::models::{AgentModelsList, AgentModelsSync};
use crate::command::provider::{
    AgentProviderAuthLogin, AgentProviderAuthLogout, AgentProviderAuthStatus,
};
use crate::command::session::{AgentSessionClear, AgentSessionInspect, AgentSessionList};
use agnt_core::session::StoreType;

pub struct AgentPlugin {
    /// The shared tokio runtime, or the error that prevented its construction.
    /// Stored as a `Result` so `new()` stays infallible; the error surfaces on
    /// first use via `runtime()`.
    runtime: Result<tokio::runtime::Runtime, String>,
    store_type: StoreType,
}

impl AgentPlugin {
    /// Builds the multi-threaded tokio runtime used to bridge the sync plugin
    /// boundary. Returns an error string if construction fails.
    fn build_runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create tokio runtime for AgentPlugin: {e}"))
    }

    /// Returns a reference to the shared tokio runtime, or an error if the
    /// runtime failed to construct.
    pub fn runtime(&self) -> Result<&tokio::runtime::Runtime, nu_protocol::LabeledError> {
        self.runtime
            .as_ref()
            .map_err(|e| nu_protocol::LabeledError::new(e.to_string()))
    }

    /// Resolve store type from CLI `--store` flag, falling back to plugin default.
    pub fn resolve_store_type(
        &self,
        call: &nu_plugin::EvaluatedCall,
    ) -> Result<StoreType, nu_protocol::LabeledError> {
        if let Some(store_str) = call.get_flag::<String>("store")? {
            store_str.parse().map_err(|e: String| {
                nu_protocol::LabeledError::new(format!("Invalid --store value: {e}"))
                    .with_label("expected 'sqlite', 'jsonl', or 'memory'", call.head)
            })
        } else {
            Ok(self.store_type)
        }
    }
}

impl Default for AgentPlugin {
    /// Creates a new AgentPlugin. No session store is created at construction
    /// time — each command opens the rig-memory store it needs. This avoids
    /// panics during `plugin add` when the cache directory may not exist.
    fn default() -> Self {
        Self {
            runtime: Self::build_runtime(),
            store_type: StoreType::Sqlite,
        }
    }
}

impl Plugin for AgentPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![
            Box::new(Agent),
            Box::new(AgentConfigInit),
            Box::new(AgentModelsSync),
            Box::new(AgentModelsList),
            Box::new(AgentProviderAuthLogin),
            Box::new(AgentProviderAuthLogout),
            Box::new(AgentProviderAuthStatus),
            Box::new(AgentAuthMcpLogin),
            Box::new(AgentAuthMcpLogout),
            Box::new(AgentAuthMcpStatus),
            Box::new(AgentSessionClear),
            Box::new(AgentSessionInspect),
            Box::new(AgentSessionList),
        ]
    }
}
