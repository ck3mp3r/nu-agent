//! Dev-only wiremock harness shared by the nu-agent test suites.
//!
//! Three OpenAI-compatible SSE stub stacks grew up independently in the
//! `agnt-core` and `agnt-acp` test trees. This crate is the single shared
//! implementation they are meant to collapse into.
//!
//! The crate is development-only. No production crate depends on it, and it
//! must never appear in a production `[dependencies]` table.
//!
//! Two wiremock limits shape what this harness can and cannot do:
//!
//! - There is no per-chunk streaming control. An SSE response is one complete
//!   body plus the `content-type: text/event-stream` header; wiremock writes
//!   the whole body in one write. A test that must observe the client between
//!   frames cannot use this harness.
//! - There is no mid-stream TCP fault injection. A connection cannot be reset
//!   after the first frame. A test that needs byte-level write control stays
//!   hand-rolled raw-TCP.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// region:    --- Re-exports

pub use wiremock::matchers;
pub use wiremock::{Mock, MockServer, Request, ResponseTemplate};

// endregion: --- Re-exports

// region:    --- Builders

/// Build the SSE body for a text completion.
///
/// The body holds one `content` delta per 20-character chunk, then a terminal
/// chunk with `finish_reason: "stop"` and the usage record, then `data: [DONE]`.
///
/// The frames are written with explicit format strings instead of
/// `serde_json::json!` plus `Display`. `serde_json` preserves object key order
/// only when its `preserve_order` feature is enabled, and that feature is
/// enabled by other crates in the workspace, not by this one. Format strings
/// keep the bytes stable no matter which crate set is being built.
pub fn sse_text_body(text: &str) -> String {
    let mut body = String::new();
    for chunk in text.chars().collect::<Vec<_>>().chunks(20) {
        let chunk: String = chunk.iter().collect();
        body.push_str(&format!(
            "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\n",
            json_string(&chunk)
        ));
    }
    body.push_str(STOP_FRAME);
    body.push_str(DONE_FRAME);
    body
}

/// Build the SSE body for a tool call.
///
/// The body holds three frames — the tool-call delta with an empty argument
/// string, the argument delta, and the terminal `finish_reason: "tool_calls"`
/// chunk with the usage record — then `data: [DONE]`.
///
/// `id` and `name` are interpolated verbatim, matching the reference stacks.
/// `args` is a JSON document and is encoded as a JSON string literal.
pub fn sse_tool_call_body(id: &str, name: &str, args: &str) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{id}\",\"type\":\"function\",\"function\":{{\"name\":\"{name}\",\"arguments\":\"\"}}}}]}},\"finish_reason\":null}}]}}\n\n"
    ));
    body.push_str(&format!(
        "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":{}}}}}]}},\"finish_reason\":null}}]}}\n\n",
        json_string(args)
    ));
    body.push_str(TOOL_CALLS_FRAME);
    body.push_str(DONE_FRAME);
    body
}

/// Wrap `body` in the 200 `text/event-stream` response the OpenAI-compatible
/// clients expect.
pub fn sse_response_template(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .append_header("content-type", "text/event-stream")
        .set_body_bytes(body.into_bytes())
}

// endregion: --- Builders

// region:    --- Mount helpers

/// Answer every `POST` with the text completion body for `text`.
///
/// `delay` postpones the response, which is how a test observes a client that
/// is still waiting on the provider.
pub async fn mount_text_response(server: &MockServer, text: &str, delay: Option<Duration>) {
    let mut template = sse_response_template(sse_text_body(text));
    if let Some(delay) = delay {
        template = template.set_delay(delay);
    }
    Mock::given(matchers::method("POST"))
        .respond_with(template)
        .mount(server)
        .await;
}

/// Answer the first `POST` with a tool call and every later `POST` with
/// `final_text`.
///
/// The agent runs one completion per tool round-trip, so the first request is
/// the one that must produce the call and the second is the one that sees the
/// tool result.
pub async fn mount_tool_then_text(
    server: &MockServer,
    tool_name: &str,
    arguments: serde_json::Value,
    final_text: &str,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let tool_body = sse_tool_call_body("call_0", tool_name, &arguments.to_string());
    let text_body = sse_text_body(final_text);
    Mock::given(matchers::method("POST"))
        .respond_with(move |_request: &Request| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let body = if call == 0 {
                tool_body.clone()
            } else {
                text_body.clone()
            };
            sse_response_template(body)
        })
        .mount(server)
        .await;
}

/// Answer every request with `method` — and `path`, when given — with the SSE
/// body `body`.
pub async fn mount_sse_response(
    server: &MockServer,
    method: &str,
    path: Option<&str>,
    body: String,
) {
    let mut builder = Mock::given(matchers::method(method));
    if let Some(path) = path {
        builder = builder.and(matchers::path(path));
    }
    builder
        .respond_with(sse_response_template(body))
        .mount(server)
        .await;
}

/// Answer the first `POST` to `path` with `error_status` and `error_body`, and
/// every later `POST` with the SSE body `sse_body`.
///
/// This is the shape a retry test needs: one retryable failure, then success.
pub async fn mount_error_then_sse(
    server: &MockServer,
    path: &str,
    error_status: u16,
    error_body: &str,
    sse_body: String,
) {
    Mock::given(matchers::method("POST"))
        .and(matchers::path(path))
        .respond_with(
            ResponseTemplate::new(error_status).set_body_bytes(error_body.as_bytes().to_vec()),
        )
        .up_to_n_times(1)
        .mount(server)
        .await;
    Mock::given(matchers::method("POST"))
        .respond_with(sse_response_template(sse_body))
        .mount(server)
        .await;
}

// endregion: --- Mount helpers

// region:    --- Capture helpers

/// The number of requests the server has received so far.
pub async fn received_request_count(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .map_or(0, |requests| requests.len())
}

/// The raw bodies of every request the server has received, in arrival order.
pub async fn received_request_bodies(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .collect()
}

/// Every request the server has received, headers included.
pub async fn received_requests(server: &MockServer) -> Option<Vec<Request>> {
    server.received_requests().await
}

// endregion: --- Capture helpers

// region:    --- Boot helper

/// Start a mock server on the current runtime.
pub async fn start_server() -> MockServer {
    MockServer::start().await
}

// endregion: --- Boot helper

// region:    --- Support

/// The terminal `stop` frame of a text completion body.
const STOP_FRAME: &str = "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n";

/// The terminal `tool_calls` frame of a tool-call body.
const TOOL_CALLS_FRAME: &str = "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":15,\"total_tokens\":65}}\n\n";

/// The stream terminator.
const DONE_FRAME: &str = "data: [DONE]\n\n";

/// Encode `value` as a JSON string literal.
fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/lib.rs"]
mod lib_test;

// endregion: --- Tests
