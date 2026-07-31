use std::sync::Once;

use crate::tools::mcp::oauth_callback::{AuthError, CALLBACK_PATH, CallbackServer};

/// Install the rustls crypto provider once for all tests.
fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Helper to start a server on a random port.
async fn start_server() -> CallbackServer {
    ensure_crypto_provider();
    CallbackServer::start(0).await.expect("server should start")
}

/// Helper to build a callback URL.
fn callback_url(port: u16, code: Option<&str>, state: Option<&str>, error: Option<&str>) -> String {
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
    let query = params.join("&");
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}?{query}")
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
async fn callback_with_valid_code_and_state_returns_success() {
    let server = start_server().await;
    let port = server.port();
    let state = "valid-state-123";

    // Spawn the HTTP request in a separate task so wait_for_callback doesn't block it
    let url = callback_url(port, Some("auth-code-xyz"), Some(state), None);
    let request_handle = tokio::spawn(async move {
        reqwest::get(&url)
            .await
            .expect("HTTP request should succeed")
    });

    // Wait for the callback (this blocks until the HTTP request arrives)
    let auth_code = server
        .wait_for_callback(state, 10)
        .await
        .expect("callback should resolve");

    assert_eq!(auth_code.code, "auth-code-xyz");
    assert_eq!(auth_code.state, state);

    // Verify the HTTP response
    let resp = request_handle.await.expect("request task should complete");
    assert!(resp.status().is_success(), "expected success status");
    let body = resp.text().await.expect("should read body");
    assert!(
        body.contains("Authorization successful"),
        "expected success page, got: {body}"
    );
}

#[tokio::test]
async fn callback_with_unknown_state_returns_error_page() {
    let server = start_server().await;
    let port = server.port();

    let url = callback_url(port, Some("code"), Some("unknown-state"), None);
    let resp = reqwest::get(&url)
        .await
        .expect("HTTP request should succeed");
    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");

    let body = resp.text().await.expect("should read body");
    assert!(
        body.contains("Invalid state parameter"),
        "expected CSRF error page, got: {body}"
    );
}

#[tokio::test]
async fn callback_with_missing_state_returns_error_page() {
    let server = start_server().await;
    let port = server.port();

    let url = callback_url(port, Some("code"), None, None);
    let resp = reqwest::get(&url)
        .await
        .expect("HTTP request should succeed");
    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");

    let body = resp.text().await.expect("should read body");
    assert!(
        body.contains("Missing state parameter"),
        "expected missing state error page, got: {body}"
    );
}

#[tokio::test]
async fn callback_with_error_param_rejects_pending() {
    let server = start_server().await;
    let port = server.port();
    let state = "error-state-456";

    // Spawn the HTTP request in a separate task
    let url = callback_url(port, None, Some(state), Some("access_denied"));
    let request_handle = tokio::spawn(async move {
        reqwest::get(&url)
            .await
            .expect("HTTP request should succeed")
    });

    // Wait for the callback — should fail with OAuth error
    let result = server.wait_for_callback(state, 10).await;
    assert!(result.is_err(), "expected error from rejected pending auth");
    match result {
        Err(AuthError::OAuthError(msg)) => {
            assert!(
                msg.contains("access_denied"),
                "expected access_denied error, got: {msg}"
            );
        }
        other => panic!("expected OAuthError, got: {other:?}"),
    }

    // Verify the HTTP response
    let resp = request_handle.await.expect("request task should complete");
    assert_eq!(resp.status().as_u16(), 400, "expected 400 Bad Request");
    let body = resp.text().await.expect("should read body");
    assert!(
        body.contains("OAuth error"),
        "expected OAuth error page, got: {body}"
    );
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
    let port = server.port();

    // No pending auths — stop_if_idle should stop the server
    server.stop_if_idle();

    // Verify the server is no longer accepting connections
    let url = format!("http://127.0.0.1:{port}{CALLBACK_PATH}?code=x&state=y");
    let result = reqwest::get(&url).await;
    assert!(
        result.is_err(),
        "expected connection error after server stopped"
    );
}
