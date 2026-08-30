use super::Agent;
use nu_parser::parse;
use nu_plugin::EvaluatedCall;
use nu_protocol::{
    ParseError, PipelineData, ShellError, Span, Spanned, Value,
    engine::{Call, Command, EngineState, Stack, StateWorkingSet},
};
use tempfile::TempDir;

/// Helper to create an Agent for testing. The Agent no longer holds a store;
/// this is kept for tests that only check signature/name properties.
pub(super) fn create_test_agent() -> (Agent, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let agent = Agent;
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
