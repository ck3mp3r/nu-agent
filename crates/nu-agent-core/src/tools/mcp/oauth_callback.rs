use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpSocket;
use tokio::sync::Mutex;
use url::form_urlencoded;

use crate::bus::OneshotTx;

/// Path for the OAuth callback endpoint.
pub const CALLBACK_PATH: &str = "/mcp/oauth/callback";

/// Default timeout for waiting for a callback (2 minutes).
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Authorization code received from the OAuth provider callback.
#[derive(Debug, Clone)]
pub struct AuthCode {
    pub code: String,
    pub state: String,
}

/// Errors that can occur during OAuth callback handling.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("OAuth error: {0}")]
    OAuthError(String),
    #[error("CSRF validation failed: unknown state parameter")]
    CsrfValidationFailed,
    #[error("Missing authorization code in callback")]
    MissingCode,
    #[error("Timeout waiting for callback")]
    Timeout,
    #[error("Server error: {0}")]
    Server(String),
}

struct PendingAuth {
    tx: OneshotTx<Result<AuthCode, AuthError>>,
}

/// A loopback HTTP server that receives OAuth authorization code callbacks.
///
/// Binds to `127.0.0.1` on a configurable port and listens for callbacks
/// at `{CALLBACK_PATH}`. Supports state parameter validation (CSRF protection),
/// timeout per pending auth, and auto-stop when idle.
pub struct CallbackServer {
    pending: Arc<Mutex<HashMap<String, PendingAuth>>>,
    server_handle: Option<tokio::task::JoinHandle<()>>,
    port: u16,
}

impl CallbackServer {
    /// Start the callback server on `127.0.0.1:{port}`.
    ///
    /// If `port` is `0`, the OS will assign an available port. Use [`port()`](Self::port)
    /// to retrieve the actual port after starting.
    pub async fn start(port: u16) -> Result<Self, AuthError> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

        let socket = TcpSocket::new_v4()
            .map_err(|e| AuthError::Server(format!("Failed to create socket: {e}")))?;
        socket
            .set_reuseaddr(true)
            .map_err(|e| AuthError::Server(format!("Failed to set SO_REUSEADDR: {e}")))?;
        socket
            .bind(addr)
            .map_err(|e| AuthError::Server(format!("Failed to bind to 127.0.0.1:{port}. Another process may be intercepting OAuth callbacks: {e}")))?;

        let listener = socket
            .listen(128)
            .map_err(|e| AuthError::Server(format!("Failed to listen: {e}")))?;

        let actual_port = listener
            .local_addr()
            .map_err(|e| AuthError::Server(format!("Failed to get local address: {e}")))?
            .port();

        let pending: Arc<Mutex<HashMap<String, PendingAuth>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = pending.clone();

        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let pending = pending_clone.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = service_fn(move |req| handle_request(req, pending.clone()));
                    if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                        log::error!("HTTP connection error: {e}");
                    }
                });
            }
        });

        Ok(Self {
            pending,
            server_handle: Some(handle),
            port: actual_port,
        })
    }

    /// Register a pending auth for the given `state` and wait for the callback.
    ///
    /// Returns the [`AuthCode`] if the callback is received within `timeout_secs`,
    /// or [`AuthError::Timeout`] if the timeout expires.
    pub async fn wait_for_callback(
        &self,
        state: &str,
        timeout_secs: u64,
    ) -> Result<AuthCode, AuthError> {
        let (tx, rx) = OneshotTx::<Result<AuthCode, AuthError>>::channel("oauth");

        {
            let mut pending = self.pending.lock().await;
            pending.insert(state.to_string(), PendingAuth { tx });
        }

        let timeout = Duration::from_secs(timeout_secs);
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                // Sender was dropped without sending
                Err(AuthError::Server(
                    "Callback channel closed unexpectedly".to_string(),
                ))
            }
            Err(_) => {
                // Timeout — clean up the pending entry
                let mut pending = self.pending.lock().await;
                pending.remove(state);
                Err(AuthError::Timeout)
            }
        }
    }

    /// Stop the server if there are no pending auths waiting for callbacks.
    ///
    /// Uses a non-blocking lock to check the pending map. If the lock is
    /// contended (another task is modifying the map), the check is skipped.
    pub fn stop_if_idle(&mut self) {
        if let Ok(pending) = self.pending.try_lock()
            && pending.is_empty()
            && let Some(handle) = self.server_handle.take()
        {
            handle.abort();
        }
    }

    /// The port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for CallbackServer {
    fn drop(&mut self) {
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
        }
    }
}

async fn handle_request(
    req: Request<Incoming>,
    pending: Arc<Mutex<HashMap<String, PendingAuth>>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Only handle the callback path
    if req.uri().path() != CALLBACK_PATH {
        return not_found_page().or_else(internal_error_page);
    }

    let query = req.uri().query().unwrap_or("");
    let params: HashMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();

    // Check for OAuth error parameter
    if let Some(error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(|s| s.as_str())
            .unwrap_or("");
        let state = params.get("state").map(|s| s.as_str()).unwrap_or("");

        // Reject pending auth if state matches
        if !state.is_empty() {
            let mut pending_lock = pending.lock().await;
            if let Some(auth) = pending_lock.remove(state) {
                let _ = auth
                    .tx
                    .send(Err(AuthError::OAuthError(format!("{error}: {desc}"))));
            }
        }

        return error_page(&format!("OAuth error: {error}")).or_else(internal_error_page);
    }

    // Validate state parameter
    let state = match params.get("state") {
        Some(s) => s,
        None => {
            return error_page("Missing state parameter").or_else(internal_error_page);
        }
    };

    // Check if state is known (CSRF protection)
    let mut pending_lock = pending.lock().await;
    let auth = match pending_lock.remove(state) {
        Some(a) => a,
        None => {
            return error_page("Invalid state parameter — possible CSRF attack")
                .or_else(internal_error_page);
        }
    };

    // Validate code parameter
    let code = match params.get("code") {
        Some(c) => c,
        None => {
            let _ = auth.tx.send(Err(AuthError::MissingCode));
            return error_page("Missing authorization code").or_else(internal_error_page);
        }
    };

    // Resolve the pending auth
    let auth_code = AuthCode {
        code: code.clone(),
        state: state.clone(),
    };
    let _ = auth.tx.send(Ok(auth_code));

    success_page().or_else(internal_error_page)
}

fn success_page() -> Result<Response<Full<Bytes>>, http::Error> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html")
        .body(Full::new(Bytes::from_static(
            b"<html><body><h1>Authorization successful</h1><p>You can close this window and return to the application.</p></body></html>",
        )))
}

fn error_page(message: &str) -> Result<Response<Full<Bytes>>, http::Error> {
    let body = format!(
        "<html><body><h1>Authorization failed</h1><p>{}</p></body></html>",
        message
    );
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header("Content-Type", "text/html")
        .body(Full::new(Bytes::from(body)))
}

fn not_found_page() -> Result<Response<Full<Bytes>>, http::Error> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Content-Type", "text/html")
        .body(Full::new(Bytes::from_static(
            b"<html><body><h1>Not found</h1></body></html>",
        )))
}

/// Fallback served if a static response builder unexpectedly fails. Response
/// builders only error on invalid HTTP values; these use static constants, so a
/// build failure is effectively impossible — but we degrade to a plain 500
/// rather than panicking.
fn internal_error_page(_e: http::Error) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("Content-Type", "text/html")
        .body(Full::new(Bytes::from_static(
            b"<html><body><h1>Internal server error</h1></body></html>",
        )))
        .unwrap_or_else(|_| {
            // Even the fallback failed to build — degrade to a minimal 500.
            let mut resp = Response::new(Full::new(Bytes::new()));
            *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            resp
        }))
}

#[cfg(test)]
#[path = "oauth_callback_test.rs"]
mod oauth_callback_test;
