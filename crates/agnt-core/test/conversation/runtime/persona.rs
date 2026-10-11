// ========================================================================
// Phase K: PersonaState characterisation tests
// ========================================================================

#[test]
fn persona_state_agent_identity_none_by_default() {
    let persona_state = crate::conversation::state::persona::PersonaState::new(
        None, None, None, None, None, None, None,
    );
    assert!(persona_state.agent_identity().is_none());
}

#[test]
fn persona_state_agent_description_none_by_default() {
    let persona_state = crate::conversation::state::persona::PersonaState::new(
        None, None, None, None, None, None, None,
    );
    assert!(persona_state.agent_description().is_none());
}
