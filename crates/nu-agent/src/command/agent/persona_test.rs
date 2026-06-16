#[test]
#[ignore = "integration test — requires EvaluatedCall construction; real coverage is in the orchestrator_test.rs which exercises resolve_persona indirectly via fn run()"]
fn resolve_persona_returns_empty_when_no_agent_configured() {
    // Would call resolve_persona(None, None, &AgentsConfig::default(), engine, call, &mut config, false)
    // and assert persona_resolution.persona.is_none() and agent_identity.is_none()
}
