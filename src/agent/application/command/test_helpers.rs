use crate::agent::application::command::{Agent, EngineConfigInterface};
use crate::plugin::RuntimeCtx;
use crate::session::SessionStore;
use nu_parser::parse;
use nu_plugin::EvaluatedCall;
use nu_protocol::{
    LabeledError, ParseError, PipelineData, ShellError, Span, Spanned, Value,
    engine::{Call, Command, EngineState, Stack, StateWorkingSet},
};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Helper to create an Agent with a test SessionStore
pub(super) fn create_test_agent() -> (Agent, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store = SessionStore::new_with_cache_dir(temp_dir.path().to_path_buf());
    let agent = Agent::new(store, RuntimeCtx::new());
    (agent, temp_dir)
}

#[derive(Clone)]
pub(super) struct ParserHarnessCommand {
    pub(super) signature: nu_protocol::Signature,
}

impl Command for ParserHarnessCommand {
    fn name(&self) -> &str {
        "agent"
    }

    fn signature(&self) -> nu_protocol::Signature {
        self.signature.clone()
    }

    fn description(&self) -> &str {
        "parser harness command"
    }

    fn run(
        &self,
        _engine_state: &EngineState,
        _stack: &mut Stack,
        _call: &Call,
        _input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        Ok(PipelineData::empty())
    }
}

pub(super) fn parse_agent_invocation_with_signature(
    sig: nu_protocol::Signature,
    invocation: &str,
) -> Vec<ParseError> {
    let mut engine_state = EngineState::new();
    let mut working_set = StateWorkingSet::new(&engine_state);
    let decl_id = working_set.add_decl(Box::new(ParserHarnessCommand { signature: sig }));
    working_set.use_decls(vec![(b"agent".to_vec(), decl_id)]);

    let _ = parse(&mut working_set, None, invocation.as_bytes(), false);
    let parse_errors = working_set.parse_errors.clone();
    let delta = working_set.render();
    engine_state
        .merge_delta(delta)
        .expect("merge parser harness state");
    parse_errors
}

/// Helper to create an EvaluatedCall with named arguments for testing
pub(super) fn create_test_call(flags: Vec<(&str, Value)>) -> EvaluatedCall {
    let span = Span::test_data();

    // Convert flags to the format EvaluatedCall expects
    let named: Vec<(Spanned<String>, Option<Value>)> = flags
        .into_iter()
        .map(|(name, value)| {
            let spanned_name = Spanned {
                item: name.to_string(),
                span,
            };
            (spanned_name, Some(value))
        })
        .collect();

    EvaluatedCall {
        head: span,
        positional: vec![],
        named,
    }
}

// ============================================================================
// MockEngineInterface - Test helper for config resolution tests
// ============================================================================

/// Mock implementation of EngineConfigInterface for testing config resolution
///
/// Allows setting a predetermined return value for get_plugin_config()
/// to test various config scenarios without requiring a real Nushell engine.
pub(super) struct MockEngineInterface {
    plugin_config: Arc<Mutex<Option<Value>>>,
}

impl MockEngineInterface {
    /// Create a new mock with no plugin config
    pub(super) fn new() -> Self {
        Self {
            plugin_config: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a mock that returns the given plugin config
    pub(super) fn with_config(config: Value) -> Self {
        Self {
            plugin_config: Arc::new(Mutex::new(Some(config))),
        }
    }

    /// Set the plugin config to return
    pub(super) fn set_config(&self, config: Option<Value>) {
        *self.plugin_config.lock().unwrap() = config;
    }
}

impl EngineConfigInterface for MockEngineInterface {
    fn get_plugin_config(&self) -> Result<Option<Value>, LabeledError> {
        Ok(self.plugin_config.lock().unwrap().clone())
    }
}
