use super::*;

// ========================================================================
// Structured messages tests
// ========================================================================

#[test]
fn build_system_preamble_joins_non_empty_parts() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        Some("preamble text"),
        None,
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert!(text.contains("preamble text"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));
    Ok(())
}

#[test]
fn build_system_preamble_includes_cwd_first() -> Result<()> {
    let result = super::super::build_system_preamble(
        Some("Working directory: /tmp/project"),
        Some("config preamble"),
        None,
        None,
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert!(text.contains("Working directory: /tmp/project"));

    // cwd must be the first part the LLM sees
    let cwd_pos = text
        .find("Working directory: /tmp/project")
        .ok_or("cwd marker missing")?;
    let config_pos = text
        .find("config preamble")
        .ok_or("config preamble missing")?;
    assert!(
        cwd_pos < config_pos,
        "cwd should come before config preamble"
    );
    Ok(())
}

#[test]
fn build_system_preamble_returns_none_when_all_empty() {
    let result = super::super::build_system_preamble(None, None, None, None, None, None, None);
    assert!(result.is_none());
}

#[test]
fn build_system_preamble_handles_partial_inputs() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        Some("preamble"),
        None,
        None,
        None,
        Some("agents"),
        None,
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert!(text.contains("preamble"));
    assert!(text.contains("agents"));
    Ok(())
}

#[test]
fn build_system_preamble_includes_persona_in_correct_position() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        Some("config preamble"),
        Some("agent persona"),
        None,
        Some("context text"),
        Some("agents chain"),
        Some("available skills"),
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;

    // Verify all parts are present
    assert!(text.contains("config preamble"));
    assert!(text.contains("agent persona"));
    assert!(text.contains("context text"));
    assert!(text.contains("agents chain"));
    assert!(text.contains("available skills"));

    // Verify persona appears between config preamble and context
    let config_pos = text
        .find("config preamble")
        .ok_or("config preamble missing")?;
    let persona_pos = text.find("agent persona").ok_or("agent persona missing")?;
    let context_pos = text.find("context text").ok_or("context text missing")?;

    assert!(
        config_pos < persona_pos,
        "config preamble should come before persona"
    );
    assert!(
        persona_pos < context_pos,
        "persona should come before context"
    );
    Ok(())
}

#[test]
fn build_system_preamble_persona_only() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        None,
        Some("persona only"),
        None,
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert_eq!(text, "persona only");
    Ok(())
}

#[test]
fn build_system_preamble_includes_sub_agent_instruction() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        None,
        Some("persona"),
        Some("sub-agent instruction"),
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert!(text.contains("persona"));
    assert!(text.contains("sub-agent instruction"));

    // sub-agent instruction should come after persona
    let persona_pos = text.find("persona").ok_or("persona marker missing")?;
    let instruction_pos = text
        .find("sub-agent instruction")
        .ok_or("sub-agent instruction marker missing")?;
    assert!(
        persona_pos < instruction_pos,
        "sub-agent instruction should come after persona"
    );
    Ok(())
}

#[test]
fn build_system_preamble_sub_agent_instruction_only() -> Result<()> {
    let result = super::super::build_system_preamble(
        None,
        None,
        None,
        Some("you are a sub-agent"),
        None,
        None,
        None,
    );

    assert!(result.is_some());
    let text = result.ok_or("build_system_preamble should yield a Some")?;
    assert_eq!(text, "you are a sub-agent");
    Ok(())
}
