//! The session manager's types: the live session, its channel hands, the
//! runtime-builder seam, the client filesystem state, and the manager itself.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};

use agent_client_protocol::schema::v1::{FileSystemCapabilities, StopReason};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_runtime::bus::{Bus, CancelTx, Control, Lifecycle, Narration, Stream};
use agnt_runtime::config::ModelsCache;
use agnt_runtime::config::Paths;
use agnt_runtime::conversation::runtime::AgentConversationRuntime;
use agnt_runtime::session::RigStoreConfig;
use chrono::{DateTime, Utc};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::Result;
use crate::runtime::{AcpRuntimeParams, build_acp_runtime};

/// How a prompt turn ended, as the ACP layer sees it.
///
/// This is the ACP boundary's own vocabulary: the turn's
/// [`TurnEnd`](agnt_runtime::conversation::turn::TurnEnd) carries the executor's
/// classification, and the session maps its variant onto the stop category the
/// ACP client is told about. No message text is inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCategory {
    /// The turn finished normally.
    Completed,
    /// The client cancelled the turn via `session/cancel`.
    Cancelled,
    /// The model hit its output-token ceiling.
    MaxTokens,
    /// The turn hit the configured tool-turn limit.
    MaxTurns,
}

/// Map a finished turn to the ACP stop reason reported to the client.
pub fn map_stop_category_to_stop_reason(outcome: StopCategory) -> StopReason {
    match outcome {
        StopCategory::Completed => StopReason::EndTurn,
        StopCategory::Cancelled => StopReason::Cancelled,
        StopCategory::MaxTokens => StopReason::MaxTokens,
        StopCategory::MaxTurns => StopReason::MaxTurnRequests,
    }
}

/// One live ACP session.
///
/// The session is shared behind an `Arc`, so `cancel()` can reach the bus and
/// the cancel flag without taking the runtime lock that an in-flight turn
/// holds. Only the runtime is behind a mutex.
pub struct AcpSession {
    pub(super) id: String,
    pub(super) cwd: PathBuf,
    pub(super) created_at: DateTime<Utc>,
    pub(super) cancel_requested: Arc<AtomicBool>,
    pub(super) bus: Bus,
    pub(super) runtime: Arc<AsyncMutex<AgentConversationRuntime>>,
    /// The session's cumulative LLM usage, shared with the runtime's cells.
    ///
    /// Held here so the close/delete paths can report it without taking the
    /// runtime lock an in-flight turn holds.
    pub(super) usage: agnt_runtime::hook::TurnUsage,
}

/// The channel hands of one live ACP session.
///
/// A caller that publishes to or subscribes to a session channel takes the one
/// it needs; the session's bus stays private to the session.
pub struct SessionChannels<'a> {
    pub stream: &'a Stream,
    pub narration: &'a Narration,
    pub lifecycle: &'a Lifecycle,
    pub control: &'a Control,
    pub cancel: &'a CancelTx,
}

impl AcpSession {
    /// The working directory the session was created with.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The session's channel hands, reachable without locking the runtime.
    pub fn channels(&self) -> SessionChannels<'_> {
        SessionChannels {
            stream: self.bus.stream(),
            narration: self.bus.narration(),
            lifecycle: self.bus.lifecycle(),
            control: self.bus.control(),
            cancel: self.bus.cancel(),
        }
    }

    /// The session's cumulative LLM usage, readable without the runtime lock.
    pub(crate) fn usage(&self) -> agnt_runtime::hook::TurnUsageTotals {
        self.usage.totals()
    }
}

/// Builds the conversation runtime of one ACP session.
///
/// The ACP boundary owns this seam so a session manager can be driven with a
/// substitute runtime builder without changing the production path, which
/// builds through [`build_acp_runtime`].
pub trait RuntimeBuilder: Send + Sync {
    /// Build the runtime for `params`.
    fn build(
        &self,
        params: AcpRuntimeParams,
    ) -> impl Future<Output = Result<AgentConversationRuntime>> + Send;
}

/// Production [`RuntimeBuilder`]: builds the runtime from config.toml.
pub struct DefaultRuntimeBuilder;

impl RuntimeBuilder for DefaultRuntimeBuilder {
    async fn build(&self, params: AcpRuntimeParams) -> Result<AgentConversationRuntime> {
        build_acp_runtime(params).await
    }
}

/// The ACP client's filesystem capabilities and the connection that reaches it.
///
/// Captured from the `initialize` request. A session runtime uses it to route
/// file reads and writes through the client instead of the local filesystem.
#[derive(Clone)]
pub struct ClientFsState {
    /// The connection back to the client, used to send `fs/*` requests.
    pub connection: ConnectionTo<Client>,
    /// The filesystem capabilities the client advertised at `initialize`.
    pub capabilities: FileSystemCapabilities,
}

/// Registry of the live sessions of one ACP connection.
///
/// Every session shares `rig`, so a session opened here can later be resumed
/// from the same store by another connection. The map itself is behind a plain
/// mutex that is only ever held for a lookup or an insert — never across an
/// await — so a long-running turn cannot block `cancel()`.
pub struct AcpSessionManager<R: RuntimeBuilder> {
    pub(super) sessions: StdMutex<HashMap<String, Arc<AcpSession>>>,
    /// How the rig-memory store of a session is made. The agent writes its
    /// conversation there, so `session/load` and `session/resume` read it.
    pub(super) rig: RigStoreConfig,
    /// The resolved application paths: the config file every session runtime is
    /// built from, and the persona directory.
    pub(super) paths: Paths,
    pub(super) runtime_handle: tokio::runtime::Handle,
    pub(super) runtime_builder: R,
    pub(super) models_cache: Option<Arc<ModelsCache>>,
    pub(super) client_fs: Arc<StdMutex<Option<ClientFsState>>>,
    pub(super) agent_name: Option<String>,
    pub(super) hooks: Vec<agnt_runtime::config::ResolvedHook>,
}
