//! Shared fixtures for the journey integration test modules.
//!
//! Provides `JourneyHarness`, the mock tool infrastructure, the wiremock SSE
//! body helpers, and the message assertion helpers used by every journey
//! scenario file.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nu_protocol::LabeledError;
use rig::test_utils::MockCompletionModel;

use super::super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::super::test_utils::{BusEventCollector, MockResolver, test_config};
use super::super::*;
use crate::conversation::providers::CachedProviderClient;
use crate::conversation::state::memory::MemoryState;
use crate::session::{FsSessionStore, StoreEntry};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::tools::handler::builtin_tool::ToolRenderRegistry;
use crate::types::Message;

pub(super) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn default_tool_infra(
    handle: rig::tool::server::ToolServerHandle,
    definitions: Vec<rig::completion::ToolDefinition>,
) -> ToolInfra {
    ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: definitions,
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus: crate::bus::create_bus(),
        render_registry: ToolRenderRegistry::default(),
    }
}

// ---------------------------------------------------------------------------
// SSE body helpers for wiremock integration tests
// ---------------------------------------------------------------------------

pub(super) fn sse_text_response(text: &str) -> String {
    let chunks: Vec<String> = text
        .chars()
        .collect::<Vec<_>>()
        .chunks(20)
        .map(|c| c.iter().collect::<String>())
        .map(|chunk| format!(
            "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\n",
            serde_json::to_string(&chunk).unwrap()
        ))
        .collect();

    let mut body = chunks.join("");
    body.push_str("data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n");
    body.push_str("data: [DONE]\n\n");
    body
}

pub(super) fn sse_tool_call_response(id: &str, name: &str, args: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{id}\",\"type\":\"function\",\"function\":{{\"name\":\"{name}\",\"arguments\":\"\"}}}}]}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{\"tool_calls\":[{{\"index\":0,\"function\":{{\"arguments\":{}}}}}]}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"chatcmpl-test\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}],\"usage\":{{\"prompt_tokens\":50,\"completion_tokens\":15,\"total_tokens\":65}}}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(args).unwrap()
    )
}

// ---------------------------------------------------------------------------
// JourneyHarness
// ---------------------------------------------------------------------------

pub(super) struct JourneyHarness {
    _temp_dir: tempfile::TempDir, // leading underscore keeps TempDir alive
    pub(super) memory_state: MemoryState<FsSessionStore>,
    shared_model: std::sync::Arc<std::sync::Mutex<rig::agent::ModelHandle>>,
    compaction_config: crate::conversation::compaction::CompactionConfig<FsSessionStore>,
    pub(super) session_id: &'static str,
    config: crate::config::Config,
}

impl JourneyHarness {
    pub(super) fn new(session_id: &'static str) -> Self {
        Self::new_with_config(session_id, test_config())
    }

    /// Create a harness with a custom config (e.g., to set max_tool_result_bytes).
    pub(super) fn new_with_config(session_id: &'static str, config: crate::config::Config) -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
        let bus = crate::bus::create_bus();
        let compaction_config = crate::conversation::compaction::CompactionConfig {
            compactor: crate::conversation::compaction::compactor::NuCompactor::from_shared_model(
                test_utils::shared_mock_model_handle(),
                bus.clone(),
                None,
            ),
            params: crate::compaction::CompactionParams::default(),
            threshold_tokens: None,
        };
        let memory_state = MemoryState::new(store);
        Self {
            _temp_dir: temp_dir,
            memory_state,
            shared_model: test_utils::shared_mock_model_handle(),
            compaction_config,
            session_id,
            config,
        }
    }

    /// Execute one turn. Returns the outcome plus the bus events published during
    /// the turn (as `UiEvent`s converted at the boundary).
    pub(super) async fn turn(
        &mut self,
        prompt: &str,
        model: MockCompletionModel,
        tool_infra: ToolInfra,
    ) -> (
        std::result::Result<TurnOutcome, LabeledError>,
        Vec<crate::protocol::event::UiEvent>,
    ) {
        // Wrap the scripted mock model in the shared handle so the agent (built
        // from the handle) routes to this model.
        *self.shared_model.lock().expect("model mutex poisoned") =
            rig::agent::ModelHandle::new(model);
        let mut executor = TurnExecutor::new(
            &self.config,
            &mut self.memory_state,
            tool_infra.clone(),
            Arc::clone(&self.shared_model),
            self.compaction_config.clone(),
        );
        let mut event_collector = BusEventCollector::subscribe(&tool_infra.bus);
        let outcome = executor
            .execute(
                ExecuteInput {
                    prompt: prompt.to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(self.session_id),
            )
            .await;
        let events = event_collector.drain();
        (outcome, events)
    }

    /// Raw messages from store — no repair, no filtering.
    pub(super) async fn raw_messages(&self) -> Result<Vec<Message>> {
        let entries = self
            .memory_state
            .inner_memory()
            .load_all(self.session_id)
            .await
            .map_err(|e| format!("store load: {e:?}"))?;
        Ok(entries
            .into_iter()
            .filter_map(|e| match e {
                StoreEntry::Message(m) => Some(m),
                _ => None,
            })
            .collect())
    }

    /// Starts a wiremock MockServer on the harness runtime.
    /// Caller must keep the returned MockServer alive for the duration of the test —
    /// dropping it early causes connection refused mid-stream.
    pub(super) async fn start_mock_server(
        &self,
    ) -> Result<(wiremock::MockServer, CachedProviderClient)> {
        // Start MockServer on a dedicated runtime to avoid conflicts with the harness rt.
        let server = wiremock::MockServer::start().await;
        // Use OpenAiCompletions (which targets /chat/completions) for wiremock tests.
        // This is the OpenAI-compatible completions API path, matching our wiremock setup.
        let http_client = crate::conversation::providers::build_http_client(None)
            .map_err(|e| format!("build test http client: {e}"))?;
        let openai_client = rig::providers::openai::Client::builder()
            .http_client(http_client)
            .base_url(server.uri())
            .api_key("fake-key".to_string())
            .build()
            .map_err(|e| format!("build openai client: {e:?}"))?;
        let cached = CachedProviderClient::OpenAiCompletions(openai_client.completions_api());
        Ok((server, cached))
    }

    pub(super) async fn turn_with_client(
        &mut self,
        prompt: &str,
        client: &CachedProviderClient,
        tool_infra: ToolInfra,
    ) -> Result<(
        std::result::Result<TurnOutcome, LabeledError>,
        Vec<crate::protocol::event::UiEvent>,
    )> {
        // Wrap the wiremock client's model in the shared handle so the agent
        // (built from the handle) routes to this client's model.
        *self.shared_model.lock().expect("model mutex poisoned") = client
            .build_model_handle(&self.config.model)
            .map_err(|e| format!("build model handle from cached client: {e:?}"))?;
        let mut executor = TurnExecutor::new(
            &self.config,
            &mut self.memory_state,
            tool_infra.clone(),
            Arc::clone(&self.shared_model),
            self.compaction_config.clone(),
        );
        let mut event_collector = BusEventCollector::subscribe(&tool_infra.bus);
        let outcome = executor
            .execute(
                ExecuteInput {
                    prompt: prompt.to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(self.session_id),
            )
            .await;
        let events = event_collector.drain();
        Ok((outcome, events))
    }
}

// ---------------------------------------------------------------------------
// Tool infrastructure helpers
// ---------------------------------------------------------------------------

/// No tools — for pure text turns.
pub(super) fn no_tools() -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new().run();
    default_tool_infra(handle, vec![])
}

/// Build a `MemoryState<FsSessionStore>` over a specific session-store path.
pub(super) fn memory_state_at(path: std::path::PathBuf) -> MemoryState<FsSessionStore> {
    let store = Arc::new(FsSessionStore::new(path));
    MemoryState::new(store)
}

// ---------------------------------------------------------------------------
// TestNuShellTool — simple nu__shell tool that returns a fixed result
// ---------------------------------------------------------------------------

struct TestNuShellTool {
    response: &'static str,
}

impl rig::tool::Tool for TestNuShellTool {
    const NAME: &'static str = "nu__shell";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Execute a Nushell command".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok(self.response.to_string())
    }
}

/// Register a nu__shell tool that returns a fixed result string.
pub(super) fn nu_shell_tool(response: &'static str) -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestNuShellTool { response })
        .run();
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "nu__shell".to_string(),
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    )
}

// ---------------------------------------------------------------------------
// TestTruncatingNuShellTool — nu__shell tool that applies our truncation logic
// ---------------------------------------------------------------------------

/// A nu__shell tool that goes through `truncate_tool_output` so integration
/// tests can verify the truncation threshold is respected end-to-end.
fn build_truncating_tool(
    response: &'static str,
    max_tool_result_bytes: usize,
) -> rig::tool::DynamicTool {
    rig::tool::DynamicTool::new(
        "nu__shell",
        "Execute a Nushell command",
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        move |_context, _args| {
            let output = response.to_string();
            let max_bytes = max_tool_result_bytes;
            Box::pin(async move {
                Ok(rig::tool::ToolOutput::text(
                    crate::tools::limits::truncate_tool_output(output, max_bytes),
                ))
            })
        },
    )
}

/// Register a nu__shell tool that applies truncation at `max_tool_result_bytes`.
pub(super) async fn nu_shell_tool_truncating(
    response: &'static str,
    max_tool_result_bytes: usize,
) -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new().run();
    // Register via add_dynamic_tool
    handle
        .add_dynamic_tool(build_truncating_tool(response, max_tool_result_bytes))
        .await;
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "nu__shell".to_string(),
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    )
}

// ---------------------------------------------------------------------------
// TestEchoTool — two named structs because Tool::NAME is a const
// ---------------------------------------------------------------------------

struct TestEchoTool {
    response: &'static str,
}

impl rig::tool::Tool for TestEchoTool {
    const NAME: &'static str = "test_echo";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Test echo tool".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok(self.response.to_string())
    }
}

/// Register one echo tool (test_echo) that returns a controlled string.
pub(super) fn echo_tool(response: &'static str) -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestEchoTool { response })
        .run();
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "test_echo".to_string(),
            description: "Test echo tool".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
    )
}

// ---------------------------------------------------------------------------
// TestNuShellCancellingTool — cancels the running turn after the first call
// ---------------------------------------------------------------------------

/// A `nu__shell` mock tool that cancels the running turn after producing its result.
///
/// Cancellation fires AFTER `call()` returns, so the tool result is recorded in
/// `new_messages` before the cancel event fires. The cancel takes effect at the next
/// `on_completion_call`'s `is_cancelled()` check (sub-turn 2), not mid-tool.
///
/// Using `tokio::task::yield_now()` ensures the tool result is committed to the
/// `new_messages` list before the cancel event is published.
struct TestNuShellCancellingTool {
    output: &'static str,
    bus: crate::bus::Bus,
    fired: Arc<AtomicBool>,
}

impl rig::tool::Tool for TestNuShellCancellingTool {
    const NAME: &'static str = "nu__shell";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Execute a Nushell command".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        let result = self.output.to_string();
        // Cancel AFTER the tool result is produced. The select! in FilteredToolProxy
        // has already resolved with Ok(result). The cancel event takes effect at the next
        // on_completion_call's is_cancelled() check — AFTER the tool result is recorded.
        if !self.fired.swap(true, Ordering::SeqCst) {
            tokio::task::yield_now().await;
            let _ = self
                .bus
                .cancel()
                .send(crate::bus::CancelEvent::Requested)
                .await;
        }
        Ok(result)
    }
}

/// Register a nu__shell tool that cancels the running turn after its first invocation.
///
/// The `bus` is the same bus the turn executor uses for cancellation.
pub(super) fn nu_shell_cancelling_tool(output: &'static str, bus: crate::bus::Bus) -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new()
        .tool(TestNuShellCancellingTool {
            output,
            bus: bus.clone(),
            fired: Arc::new(AtomicBool::new(false)),
        })
        .run();
    let mut infra = default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "nu__shell".to_string(),
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    );
    infra.bus = bus;
    infra
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

pub(super) fn assert_user_text(msg: &Message, expected: &str) {
    let Message::User { content } = msg else {
        panic!("expected User message, got: {msg:?}");
    };
    let text = content.iter().find_map(|c| {
        if let rig::message::UserContent::Text(t) = c {
            Some(t.text.as_str())
        } else {
            None
        }
    });
    assert_eq!(text, Some(expected), "user text mismatch");
}

pub(super) fn assert_assistant_text_contains(msg: &Message, needle: &str) {
    let Message::Assistant { content, .. } = msg else {
        panic!("expected Assistant message, got: {msg:?}");
    };
    let text = content.iter().find_map(|c| {
        if let rig::message::AssistantContent::Text(t) = c {
            Some(t.text.as_str())
        } else {
            None
        }
    });
    assert!(
        text.is_some_and(|t| t.contains(needle)),
        "assistant text {text:?} does not contain {needle:?}"
    );
}

pub(super) fn assert_tool_call_in_msg(
    msg: &Message,
    expected_id: &str,
    expected_name: &str,
) -> Result<()> {
    let Message::Assistant { content, .. } = msg else {
        panic!("expected Assistant message for tool call, got: {msg:?}");
    };
    let tc = content.iter().find_map(|c| {
        if let rig::message::AssistantContent::ToolCall(tc) = c {
            Some(tc)
        } else {
            None
        }
    });
    let tc = tc.ok_or("no ToolCall content in Assistant message")?;
    assert_eq!(tc.id.as_str(), expected_id, "tool call id mismatch");
    assert_eq!(tc.function.name, expected_name, "tool call name mismatch");
    Ok(())
}

pub(super) fn assert_tool_result_in_msg(
    msg: &Message,
    expected_id: &str,
    content_contains: &str,
) -> Result<()> {
    let Message::User { content } = msg else {
        panic!("expected User message for tool result, got: {msg:?}");
    };
    let tr = content
        .iter()
        .find_map(|c| {
            if let rig::message::UserContent::ToolResult(tr) = c {
                (tr.call.as_str() == expected_id).then_some(tr)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "no ToolResult for call id {expected_id} in User message"
            ))
        })?;
    let content_str = format!("{:?}", tr.content);
    assert!(
        content_str.contains(content_contains),
        "ToolResult content {content_str:?} does not contain {content_contains:?}"
    );
    Ok(())
}

/// Assert the first Text block of the ToolResult whose call id is
/// `expected_id` carries `expected` under the persisted `nu_agent_success` key.
pub(super) fn assert_tool_result_flag(
    msg: &Message,
    expected_id: &str,
    expected: Option<bool>,
) -> Result<()> {
    let Message::User { content } = msg else {
        panic!("expected User message for tool result, got: {msg:?}");
    };
    let tr = content
        .iter()
        .find_map(|c| {
            if let rig::message::UserContent::ToolResult(tr) = c {
                (tr.call.as_str() == expected_id).then_some(tr)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "no ToolResult for call id {expected_id} in User message"
            ))
        })?;
    let block = tr
        .content
        .iter()
        .find_map(|c| {
            if let rig::message::ToolResultContent::Text(t) = c {
                Some(t)
            } else {
                None
            }
        })
        .ok_or("no Text block in ToolResult")?;
    let flag = block
        .additional_params
        .as_ref()
        .and_then(|params| params.get("nu_agent_success"))
        .and_then(serde_json::Value::as_bool);
    assert_eq!(
        flag, expected,
        "persisted nu_agent_success mismatch for {expected_id}"
    );
    Ok(())
}

pub(super) fn assert_no_interrupted(msgs: &[Message]) {
    for msg in msgs {
        if let Message::User { content } = msg {
            for c in content.iter() {
                if let rig::message::UserContent::ToolResult(tr) = c {
                    let s = format!("{:?}", tr.content);
                    assert!(
                        !s.contains("[interrupted]"),
                        "synthetic [interrupted] found in ToolResult {tr:?}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Trace-logging fixture
// ---------------------------------------------------------------------------

/// A no-op `log::Log` used to enable trace logging in tests so the
/// `log_enabled!` gate in `HookChain::on_tool_result` actually runs the
/// preview construction (the code under test).
struct TraceNoopLogger;

impl log::Log for TraceNoopLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, _record: &log::Record) {}

    fn flush(&self) {}
}

static TRACE_LOGGER_INSTALL: std::sync::Once = std::sync::Once::new();

/// Install the no-op trace logger exactly once per test binary. Without a
/// logger, `log::max_level()` is Off and `log_enabled!` skips the preview
/// construction in `on_tool_result`, so the byte-slice defect is latent.
pub(super) fn install_trace_logger() {
    TRACE_LOGGER_INSTALL.call_once(|| {
        log::set_boxed_logger(Box::new(TraceNoopLogger)).ok();
        log::set_max_level(log::LevelFilter::Trace);
    });
}

/// A `nu__shell` mock tool that returns a multi-byte UTF-8 result whose byte
/// 2000 falls inside a 2-byte char — the exact shape that panicked the
/// trace-log preview in `HookChain::on_tool_result` before the fix.
pub(super) async fn nu_shell_multibyte_tool() -> ToolInfra {
    let handle = rig::tool::server::ToolServer::new().run();
    let tool = rig::tool::DynamicTool::new(
        "nu__shell",
        "Execute a Nushell command",
        serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        move |_context, _args| {
            let output = format!("{}é rest of the output", "a".repeat(1999));
            Box::pin(async move { Ok(rig::tool::ToolOutput::text(output)) })
        },
    );
    handle.add_dynamic_tool(tool).await;
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "nu__shell".to_string(),
            description: "Execute a Nushell command".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}),
        }],
    )
}
