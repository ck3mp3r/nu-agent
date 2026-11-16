//! The session manager's constructors and registry accessors.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};

use agnt_runtime::config::ModelsCache;
use agnt_runtime::config::Paths;
use agnt_runtime::session::RigStoreConfig;

use crate::error::{AcpError, Result};

use super::types::{
    AcpSession, AcpSessionManager, ClientFsState, DefaultRuntimeBuilder, RuntimeBuilder,
};

impl AcpSessionManager<DefaultRuntimeBuilder> {
    /// Create a manager whose sessions all share `rig`.
    ///
    /// `rig` is how each session's rig-memory store is made — the store the
    /// agent writes its conversation to. `paths` carries the config.toml every
    /// session runtime is built from and the persona directory; `runtime_handle`
    /// is the process tokio runtime the sessions spawn on. Session runtimes are
    /// built by [`DefaultRuntimeBuilder`].
    pub fn new(rig: RigStoreConfig, paths: Paths, runtime_handle: tokio::runtime::Handle) -> Self {
        Self::with_runtime_builder(rig, paths, runtime_handle, DefaultRuntimeBuilder)
    }
}

impl<R: RuntimeBuilder> AcpSessionManager<R> {
    /// Create a manager that builds every session runtime with `runtime_builder`.
    ///
    /// Use this to substitute the runtime builder — for example a mock builder
    /// that never touches the network.
    pub fn with_runtime_builder(
        rig: RigStoreConfig,
        paths: Paths,
        runtime_handle: tokio::runtime::Handle,
        runtime_builder: R,
    ) -> Self {
        Self {
            sessions: StdMutex::new(HashMap::new()),
            rig,
            paths,
            runtime_handle,
            runtime_builder,
            models_cache: None,
            client_fs: Arc::new(StdMutex::new(None)),
            agent_name: None,
            hooks: Vec::new(),
        }
    }

    /// Attach the user hooks every session starts a runner for.
    ///
    /// The hooks are resolved once at serve time; each session spawns its own
    /// runner on its own bus, so a hook observes that session's events only.
    pub fn with_hooks(mut self, hooks: Vec<agnt_runtime::config::ResolvedHook>) -> Self {
        self.hooks = hooks;
        self
    }

    /// Attach the models cache used to enumerate selectable models.
    ///
    /// Without a cache the model selector advertises only the configured model.
    pub fn with_models_cache(mut self, cache: Option<Arc<ModelsCache>>) -> Self {
        self.models_cache = cache;
        self
    }

    /// Attach the shared client filesystem state captured at `initialize`.
    ///
    /// The state is shared with the `initialize` handler, which fills it once
    /// the client advertises its filesystem capabilities. Without it, session
    /// runtimes fall back to the local filesystem.
    pub fn with_client_fs(mut self, state: Arc<StdMutex<Option<ClientFsState>>>) -> Self {
        self.client_fs = state;
        self
    }

    /// Attach the agent whose permissions overlay applies to every session.
    ///
    /// `None` falls back to the configured default agent. The name is carried
    /// into every [`AcpRuntimeParams`](crate::runtime::AcpRuntimeParams) this
    /// manager builds.
    pub fn with_agent_name(mut self, agent_name: Option<String>) -> Self {
        self.agent_name = agent_name;
        self
    }

    /// The live session with `id`, if any.
    pub fn get_session(&self, id: &str) -> Option<Arc<AcpSession>> {
        self.sessions().get(id).cloned()
    }

    /// The live-session registry, recovered if a holder panicked mid-update.
    pub(super) fn sessions(&self) -> MutexGuard<'_, HashMap<String, Arc<AcpSession>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The client's filesystem state, recovered if its writer panicked.
    pub(super) fn client_fs(&self) -> MutexGuard<'_, Option<ClientFsState>> {
        self.client_fs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The live session handle for `session_id`, cloned out of the registry.
    ///
    /// The registry lock is released before this returns, so the caller can
    /// hold the session for as long as it likes without blocking the registry.
    pub(super) fn session(&self, session_id: &str) -> Result<Arc<AcpSession>> {
        self.sessions()
            .get(session_id)
            .cloned()
            .ok_or_else(|| AcpError::SessionNotFound(session_id.to_string()))
    }
}
