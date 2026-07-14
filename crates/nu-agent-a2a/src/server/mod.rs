use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::{
    Router,
    routing::{delete, get, post},
};
use tokio::sync::{mpsc, oneshot};
use tower_http::cors::CorsLayer;

use crate::{A2aError, AgentCard, InMemoryTaskStore, IncomingTask, PeerCache};

pub mod handlers;
mod middleware;
mod response;

use middleware::a2a_version_middleware;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Shared application state injected into every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub task_store: Arc<InMemoryTaskStore>,
    pub agent_card: Arc<AgentCard>,
    pub incoming_tasks_tx: mpsc::Sender<IncomingTask>,
    pub peer_cache: Arc<PeerCache>,
    /// In-memory file storage for file exchange (A2A spec §6.7).
    pub files: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

// ---------------------------------------------------------------------------
// A2A Server
// ---------------------------------------------------------------------------

/// A running A2A HTTP server with graceful shutdown.
///
/// Binds to port 0 on loopback so the OS assigns a free port. The assigned
/// port is available via `self.port` / `self.local_url` after `start()`.
#[derive(Debug)]
pub struct A2aServer {
    pub port: u16,
    pub local_url: String,
    task_store: Arc<InMemoryTaskStore>,
    shutdown_handle: Option<oneshot::Sender<()>>,
    incoming_tasks_rx: Option<mpsc::Receiver<IncomingTask>>,
}

impl A2aServer {
    /// Access the server's [`InMemoryTaskStore`].
    pub fn task_store(&self) -> Arc<InMemoryTaskStore> {
        self.task_store.clone()
    }

    /// Start the A2A server on a loopback port.
    ///
    /// The server runs in a background tokio task. Drop `self`
    /// (or call `shutdown()`) to stop it.
    ///
    /// `port` of 0 means the OS assigns a random free port.
    pub async fn start(
        agent_card: AgentCard,
        peer_cache: Arc<PeerCache>,
        port: u16,
    ) -> Result<Self, A2aError> {
        // 1. Bind to all interfaces so the server is reachable on any IP
        //    that mDNS might advertise (i.e. the host's external IP).
        let bind_addr = if port == 0 {
            "0.0.0.0:0".to_string()
        } else {
            format!("0.0.0.0:{port}")
        };
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| A2aError::Internal(format!("bind failed: {e}")))?;

        // 2. Read assigned port
        let port = listener
            .local_addr()
            .map_err(|e| A2aError::Internal(format!("addr failed: {e}")))?
            .port();

        // 3. Create shutdown channel
        let (tx, rx) = oneshot::channel::<()>();

        // 4. Build shared state and event channel
        let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingTask>(64);
        let task_store: Arc<InMemoryTaskStore> = Arc::new(InMemoryTaskStore::new());
        let files: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            task_store: task_store.clone(),
            agent_card: Arc::new(agent_card),
            incoming_tasks_tx: incoming_tx,
            peer_cache,
            files,
        };

        // 5. Build axum router with all routes (spec §11.3)
        //
        // Note: axum 0.8 requires path parameters (e.g. {id}) to be the
        // complete path segment.  Colon-suffix actions like `{id}:cancel`
        // are not supported in a single segment, so we use a separate
        // segment for the colon action when a parameter is present.
        //
        // Parameter-less colon paths (message:send, tasks:list, files:upload)
        // work fine as literal matches.
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            // Message endpoints (§11.3.1)
            .route("/message:send", post(handlers::handle_tasks_send))
            .route("/message:stream", post(handlers::handle_tasks_send_stream))
            // Task endpoints
            .route("/tasks:list", post(handlers::handle_tasks_list))
            .route(
                "/tasks/{id}/subscribe",
                get(handlers::handle_tasks_subscribe),
            )
            .route("/tasks/{id}/cancel", post(handlers::handle_tasks_cancel))
            .route(
                "/tasks/{id}/complete",
                post(handlers::handle_tasks_complete),
            )
            .route("/tasks/{id}", get(handlers::handle_tasks_get))
            // Push notification configs (§11.3.2)
            .route(
                "/tasks/{id}/push-notifications/create",
                post(handlers::handle_create_push_config),
            )
            .route(
                "/tasks/{id}/push-notifications/list",
                get(handlers::handle_list_push_configs),
            )
            .route(
                "/tasks/{id}/push-notifications/delete/{config_id}",
                delete(handlers::handle_delete_push_config),
            )
            // File exchange
            .route("/files:upload", post(handlers::handle_file_upload))
            .route("/files/{id}", get(handlers::handle_file_download))
            // Agent card discovery (§8.6)
            .route(
                "/.well-known/agent-card.json",
                get(handlers::handle_agent_card),
            )
            .route(
                "/extendedAgentCard",
                get(handlers::handle_extended_agent_card),
            )
            .with_state(state.clone())
            .layer(axum::middleware::from_fn(a2a_version_middleware))
            .layer(CorsLayer::permissive());

        // 6. Serve in background
        let local_url = format!("http://127.0.0.1:{port}");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    rx.await.ok();
                })
                .await
                .expect("server failed");
        });

        // 7. Return handle
        Ok(Self {
            port,
            local_url,
            task_store,
            shutdown_handle: Some(tx),
            incoming_tasks_rx: Some(incoming_rx),
        })
    }

    /// Take the incoming task event channel receiver, if any.
    ///
    /// This can be used to receive [`IncomingTask`] events when remote agents
    /// send tasks to this server. The receiver can only be taken once; returns
    /// `None` on subsequent calls.
    pub fn take_incoming_task_receiver(&mut self) -> Option<mpsc::Receiver<IncomingTask>> {
        self.incoming_tasks_rx.take()
    }

    /// Gracefully shut down the server.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_handle.take() {
            let _ = tx.send(());
        }
    }
}
