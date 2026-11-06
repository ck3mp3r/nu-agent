use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use tokio::sync::Mutex;

use crate::tools::mcp::oauth_callback::{
    AuthError, CALLBACK_PATH, CallbackServer, PendingAuth, handle_request,
};

/// Helper to start a server on a random port.
async fn start_server() -> CallbackServer {
    CallbackServer::start(0).await.expect("server should start")
}

/// Helper to build a callback request for direct injection into `handle_request`.
fn callback_request(
    code: Option<&str>,
    state: Option<&str>,
    error: Option<&str>,
) -> Request<Full<Bytes>> {
    let mut params: Vec<String> = Vec::new();
    if let Some(c) = code {
        params.push(format!("code={}", urlencoding(c)));
    }
    if let Some(s) = state {
        params.push(format!("state={}", urlencoding(s)));
    }
    if let Some(e) = error {
        params.push(format!("error={}", urlencoding(e)));
    }
    let uri = format!("{CALLBACK_PATH}?{}", params.join("&"));
    Request::builder()
        .uri(uri)
        .body(Full::new(Bytes::new()))
        .expect("request should build")
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Wait until `wait_for_callback` has registered the pending entry for `state`.
async fn await_registered(pending: &Arc<Mutex<HashMap<String, PendingAuth>>>, state: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !pending.lock().await.contains_key(state) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "pending entry for '{state}' was not registered"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn body_text(resp: Response<Full<Bytes>>) -> String {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("body should be utf8")
}

#[tokio::test]
async fn callback_with_valid_code_and_state_returns_success() {
    let server = start_server().await;
    let pending = server.pending.clone();
    let state = "valid-state-123";

    // Register the pending auth, then inject the callback directly — no HTTP roundtrip.
    let wait_handle = tokio::spawn(async move { server.wait_for_callback(state, 10).await });
    await_registered(&pending, state).await;

    let req = callback_request(Some("auth-code-xyz"), Some(state), None);
    let resp = handle_request(req, pending)
        .await
        .expect("handle_request should not fail");

    assert!(resp.status().is_success(), "expected success status");
    let body = body_text(resp).await;
    assert!(
        body.contains("Authorization successful"),
        "expected success page, got: {body}"
    );

    let auth_code = wait_handle
        .await
        .expect("wait task should complete")
        .expect("callback should resolve");
    assert_eq!(auth_code.code, "auth-code-xyz");
    assert_eq!(auth_code.state, state);
}

#[tokio::test]
async fn callback_with_unknown_state_returns_error_page() {
    let server = start_server().await;
    let pending = server.pending.clone();

    let req = callback_request(Some("code"), Some("unknown-state"), None);
    let resp = handle_request(req, pending)
        .await
        .expect("handle_request should not fail");

    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");
    let body = body_text(resp).await;
    assert!(
        body.contains("Invalid state parameter"),
        "expected CSRF error page, got: {body}"
    );
}

#[tokio::test]
async fn callback_with_missing_state_returns_error_page() {
    let server = start_server().await;
    let pending = server.pending.clone();

    let req = callback_request(Some("code"), None, None);
    let resp = handle_request(req, pending)
        .await
        .expect("handle_request should not fail");

    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");
    let body = body_text(resp).await;
    assert!(
        body.contains("Missing state parameter"),
        "expected missing state error page, got: {body}"
    );
}

#[tokio::test]
async fn callback_with_error_param_rejects_pending() {
    let server = start_server().await;
    let pending = server.pending.clone();
    let state = "error-state-456";

    // Register the pending auth, then inject the error callback directly — no HTTP roundtrip.
    let wait_handle = tokio::spawn(async move { server.wait_for_callback(state, 10).await });
    await_registered(&pending, state).await;

    let req = callback_request(None, Some(state), Some("access_denied"));
    let resp = handle_request(req, pending)
        .await
        .expect("handle_request should not fail");

    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");
    let body = body_text(resp).await;
    assert!(
        body.contains("OAuth error"),
        "expected OAuth error page, got: {body}"
    );

    let result = wait_handle.await.expect("wait task should complete");
    match result {
        Err(AuthError::OAuthError(msg)) => {
            assert!(
                msg.contains("access_denied"),
                "expected access_denied error, got: {msg}"
            );
        }
        other => panic!("expected OAuthError, got: {other:?}"),
    }
}

#[tokio::test]
async fn wait_for_callback_times_out() {
    let server = start_server().await;

    // Use a very short timeout to test timeout behavior
    let result = server.wait_for_callback("timeout-state", 1).await;

    match result {
        Err(AuthError::Timeout) => {} // expected
        other => panic!("expected Timeout error, got: {other:?}"),
    }
}

#[tokio::test]
async fn stop_if_idle_stops_server() {
    let mut server = start_server().await;
    let addr = format!("127.0.0.1:{}", server.port());

    // No pending auths — stop_if_idle should stop the server
    server.stop_if_idle();

    // Verify the server is no longer accepting connections. The accept task is
    // aborted asynchronously, so poll until the listener socket is released.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::net::TcpStream::connect(&addr).await.is_err() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected connection error after server stopped"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}
