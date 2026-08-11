use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Router,
    routing::{delete, get, post},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

use crate::{A2aError, AgentCard, InMemoryTaskStore, IncomingTask, PeerCache};

pub mod handlers;
mod middleware;
mod response;

#[cfg(test)]
mod test;

use middleware::a2a_version_middleware;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Shared application state injected into every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub task_store: Arc<InMemoryTaskStore>,
    pub agent_card: Arc<RwLock<AgentCard>>,
    pub incoming_tasks_tx: mpsc::Sender<IncomingTask>,
    pub task_cancel_tx: mpsc::UnboundedSender<String>,
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
    agent_card: Arc<RwLock<AgentCard>>,
    shutdown_token: Option<CancellationToken>,
    incoming_tasks_rx: Option<mpsc::Receiver<IncomingTask>>,
    task_cancel_rx: Option<mpsc::UnboundedReceiver<String>>,
}

impl A2aServer {
    /// Access the server's [`InMemoryTaskStore`].
    pub fn task_store(&self) -> Arc<InMemoryTaskStore> {
        self.task_store.clone()
    }

    /// Access the server's [`AgentCard`] handle for reading/writing.
    ///
    /// The returned [`Arc`] wraps an [`RwLock`] so the card can be updated
    /// at runtime (e.g. after an agent switch) without restarting the server.
    pub fn agent_card_handle(&self) -> Arc<RwLock<AgentCard>> {
        self.agent_card.clone()
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
        let listener = tokio::net::TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| A2aError::Internal(format!("bind failed: {e}")))?;

        // 2. Read assigned port
        let port = listener
            .local_addr()
            .map_err(|e| A2aError::Internal(format!("addr failed: {e}")))?
            .port();
        let local_url = format!("http://127.0.0.1:{port}");
        let canonical_bind = format!("0.0.0.0:{port}");

        // 3. Create cancellation token for graceful shutdown
        let shutdown_token = CancellationToken::new();
        let token = shutdown_token.clone();

        // 4. Build shared state and event channel
        let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingTask>(64);
        let (cancel_tx, cancel_rx) = mpsc::unbounded_channel::<String>();
        let task_store: Arc<InMemoryTaskStore> = Arc::new(InMemoryTaskStore::new());
        let files: Arc<RwLock<HashMap<String, Vec<u8>>>> = Arc::new(RwLock::new(HashMap::new()));
        let agent_card: Arc<RwLock<AgentCard>> = Arc::new(RwLock::new(agent_card));
        let state = AppState {
            task_store: task_store.clone(),
            agent_card: agent_card.clone(),
            incoming_tasks_tx: incoming_tx,
            task_cancel_tx: cancel_tx,
            peer_cache,
            files,
        };

        // 5. Spawn restart loop
        //
        // If the server crashes (port conflict, OS error), we log the error,
        // re-bind, and restart automatically instead of dying silently.
        //
        // The first iteration uses the pre-bound `listener` so the server is
        // accepting connections before `start()` returns.  On restart we bind
        // a fresh listener inside the loop.
        tokio::spawn(async move {
            let mut current_listener = Some(listener);

            loop {
                let listener = match current_listener.take() {
                    Some(l) => l,
                    None => match tokio::net::TcpListener::bind(&canonical_bind).await {
                        Ok(l) => l,
                        Err(e) => {
                            log::error!(
                                "A2A server failed to bind port {port}: \
                                 {e}. retrying in 5s..."
                            );
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    },
                };

                // Build axum router with all routes (spec §11.3)
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

                if let Err(e) = axum::serve(listener, app)
                    .with_graceful_shutdown({
                        let token = token.clone();
                        async move { token.cancelled().await }
                    })
                    .await
                {
                    log::error!("A2A server crashed: {e}. restarting in 1s...");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                } else {
                    log::info!("A2A server shut down gracefully");
                    break;
                }
            }
        });

        // 6. Return handle
        Ok(Self {
            port,
            local_url,
            task_store,
            agent_card,
            shutdown_token: Some(shutdown_token),
            incoming_tasks_rx: Some(incoming_rx),
            task_cancel_rx: Some(cancel_rx),
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

    /// Take the task cancel event channel receiver, if any.
    ///
    /// This can be used to receive task IDs when remote agents cancel tasks
    /// that were sent to this server. The receiver can only be taken once;
    /// returns `None` on subsequent calls.
    pub fn take_task_cancel_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.task_cancel_rx.take()
    }

    /// Gracefully shut down the server.
    ///
    /// Cancels the [`CancellationToken`] which signals the background task
    /// to stop. The spawned loop exits without restarting when it detects
    /// a graceful shutdown (the `Ok` case from `axum::serve`).
    pub async fn shutdown(mut self) {
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }
        // Brief delay to allow graceful shutdown to propagate.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl Drop for A2aServer {
    fn drop(&mut self) {
        if let Some(token) = self.shutdown_token.take() {
            token.cancel();
        }
    }
}
