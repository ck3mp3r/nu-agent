use std::sync::Mutex;

use nu_plugin::{Plugin, PluginCommand};

use crate::command::agent::Agent;
use crate::command::config::AgentConfigInit;
use crate::command::mcp::{AgentAuthMcpLogin, AgentAuthMcpLogout, AgentAuthMcpStatus};
use crate::command::models::{AgentModelsList, AgentModelsSync};
use crate::command::provider::{
    AgentProviderAuthLogin, AgentProviderAuthLogout, AgentProviderAuthStatus,
};
use crate::command::session::{AgentSessionClear, AgentSessionInspect, AgentSessionList};
use nu_agent_core::session::{SessionStoreBackend, StoreError, StoreType, create_store};

pub struct AgentPlugin {
    /// The shared tokio runtime, or the error that prevented its construction.
    /// Stored as a `Result` so `new()` stays infallible; the error surfaces on
    /// first use via `runtime()` / `create_store()`.
    runtime: Result<tokio::runtime::Runtime, String>,
    store_type: StoreType,
    /// Cached in-memory session store, created once and reused for the agent's
    /// entire runtime. A fresh `:memory:` SQLite database is empty each time,
    /// so it must be created exactly once and shared across all commands.
    cached_memory_store: Mutex<Option<SessionStoreBackend>>,
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

    /// Creates the session store on first use. Called by each command's `run()`
    /// method to obtain the store lazily rather than at plugin construction.
    pub fn create_store(&self) -> Result<SessionStoreBackend, StoreError> {
        self.create_store_with(self.store_type)
    }

    /// Creates the session store with a specific store type.
    /// Used when the CLI `--store` flag overrides the default.
    pub fn create_store_with(
        &self,
        store_type: StoreType,
    ) -> Result<SessionStoreBackend, StoreError> {
        let runtime = self
            .runtime
            .as_ref()
            .map_err(|e| runtime_store_error(&e.to_string()))?;
        if store_type == StoreType::Memory {
            let mut cache = self
                .cached_memory_store
                .lock()
                .expect("cached memory store mutex poisoned");
            if cache.is_none() {
                let store = runtime.block_on(create_store(store_type))?;
                *cache = Some(store);
            }
            return cache
                .as_ref()
                .cloned()
                .ok_or_else(|| runtime_store_error("memory store not yet created"));
        }
        runtime.block_on(create_store(store_type))
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
    /// Creates a new AgentPlugin. The session store is NOT created at construction
    /// time — it is created lazily on first use via `create_store()`. This avoids
    /// panics during `plugin add` when the cache directory may not exist.
    fn default() -> Self {
        Self {
            runtime: Self::build_runtime(),
            store_type: StoreType::Sqlite,
            cached_memory_store: Mutex::new(None),
        }
    }
}

/// Converts a runtime construction error string into a `StoreError` for the
/// `create_store_with` fallible path.
fn runtime_store_error(msg: &str) -> StoreError {
    StoreError::Io(std::io::Error::other(msg))
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
