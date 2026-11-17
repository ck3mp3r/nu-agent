//! `PersonaState::switch_agent`: the persona flow the runtime shares with the
//! resolution seam.
//!
//! A switch reads one persona file, interprets its front matter, and applies the
//! result to the live state. These pins fix the observable behavior — the error
//! prefixes byte for byte, the identity rule, and the order of the state
//! mutations — so the shared-primitive refactor cannot move it.

use std::path::Path;

use super::*;
use crate::config::AgentsConfig;
use crate::conversation::managers::PersonaManager;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Write a persona file where the resolver looks first: `<dir>/.agents/`.
fn write_cwd_persona(dir: &Path, name: &str, contents: &str) -> Result<()> {
    let agents_dir = dir.join(".agents");
    std::fs::create_dir_all(&agents_dir)?;
    std::fs::write(agents_dir.join(format!("{name}.md")), contents)?;
    Ok(())
}

/// Switch a fresh state to `agent_name` with `dir` as both the cwd and the
/// agents dir.
fn switch(
    state: &mut PersonaState,
    dir: &Path,
    agent_name: &str,
) -> core::result::Result<SwitchAgentResult, String> {
    state.switch_agent(agent_name, dir, dir, &AgentsConfig::default())
}

// endregion: --- Test Support

/// WHEN the persona file carries front matter, THE switch_agent method SHALL
/// apply every front-matter field to the state and to the returned result.
#[test]
fn switch_agent_applies_the_front_matter() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(
        dir.path(),
        "my-agent",
        "---\nname: Pinned Agent\ndescription: does pinned things\nmodel: heavy\ntemperature: 0.25\nmax_tokens: 4096\nmax_tool_turns: 7\nmax_tool_calls_per_subturn: 3\nmax_tool_result_bytes: 1024\npermissions:\n  read: \"allow\"\n---\nBody text",
    )?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let result = switch(&mut state, dir.path(), "my-agent")?;

    // -- Check
    assert_eq!(result.identity, "Pinned Agent", "front-matter name wins");
    assert_eq!(result.model.as_deref(), Some("heavy"));
    assert_eq!(result.temperature, Some(0.25));
    assert_eq!(result.max_tokens, Some(4096));
    assert_eq!(result.max_tool_turns, Some(7));
    assert_eq!(result.max_tool_calls_per_subturn, Some(3));
    assert_eq!(result.max_tool_result_bytes, Some(1024));
    assert!(result.additional_params.is_none());
    assert!(
        result
            .permissions_overlay
            .as_ref()
            .is_some_and(|overlay| !overlay.is_empty()),
        "the front-matter permissions reach the result"
    );

    assert_eq!(state.agent_identity(), Some("Pinned Agent"));
    assert_eq!(state.agent_description(), Some("does pinned things"));
    assert!(state.agent_icon().is_none());
    assert_eq!(
        state.agent_persona_body().map(str::trim),
        Some("Body text"),
        "the body replaces the persona text"
    );
    assert_eq!(state.persona_body_len(), Some("Body text".len()));
    Ok(())
}

/// WHEN the persona file does not exist, THE switch_agent method SHALL fail
/// with the plain load prefix and leave the state alone.
#[test]
fn switch_agent_missing_persona_reports_the_load_leg() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let err = switch(&mut state, dir.path(), "does-not-exist")
        .err()
        .ok_or("should reject a missing persona")?;

    // -- Check
    assert!(
        err.starts_with("agent switch failed: Persona file not found"),
        "err={err}"
    );
    assert!(state.agent_identity().is_none(), "no identity was applied");
    assert!(state.agent_persona_body().is_none(), "no body was applied");
    Ok(())
}

/// WHEN the front matter is not valid YAML, THE switch_agent method SHALL fail
/// with the parse leg's prefix.
#[test]
fn switch_agent_unparsable_front_matter_reports_the_parse_leg() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "bad-yaml", "---\n[invalid: yaml:\n---\n# Body")?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let err = switch(&mut state, dir.path(), "bad-yaml")
        .err()
        .ok_or("should reject unparsable front matter")?;

    // -- Check
    assert!(
        err.starts_with(
            "agent switch failed: invalid front matter: Failed to parse YAML front matter: "
        ),
        "err={err}"
    );
    Ok(())
}

/// WHEN a front-matter field has the wrong type, THE switch_agent method SHALL
/// fail with the interpret leg's prefix.
#[test]
fn switch_agent_invalid_field_reports_the_interpret_leg() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "bad-field", "---\nname: 123\n---\nBody")?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let err = switch(&mut state, dir.path(), "bad-field")
        .err()
        .ok_or("should reject an invalid front-matter field")?;

    // -- Check
    assert_eq!(
        err,
        "agent switch failed: invalid front matter fields: Invalid field 'name': expected string, got number"
    );
    Ok(())
}

/// WHEN the persona declares no permissions, THE switch_agent method SHALL
/// return no permissions overlay.
#[test]
fn switch_agent_without_permissions_leaves_the_overlay_empty() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "plain", "---\nname: Plain\n---\nBody")?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let result = switch(&mut state, dir.path(), "plain")?;

    // -- Check
    assert!(result.permissions_overlay.is_none());
    assert_eq!(result.identity, "Plain");
    Ok(())
}

/// WHEN the front matter names no persona, THE switch_agent method SHALL use
/// the requested agent name as the identity.
#[test]
fn switch_agent_without_a_front_matter_name_falls_back_to_the_argument() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(dir.path(), "nameless", "# Nameless\nBody")?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let result = switch(&mut state, dir.path(), "nameless")?;

    // -- Check
    assert_eq!(result.identity, "nameless");
    assert_eq!(state.agent_identity(), Some("nameless"));
    assert!(result.permissions_overlay.is_none());
    Ok(())
}

/// WHEN the persona's permissions are invalid, THE switch_agent method SHALL
/// fail with the permissions prefix after it has applied the persona body and
/// identity — the mutation order the delegation must keep.
#[test]
fn switch_agent_invalid_permissions_reports_after_the_state_mutations() -> Result<()> {
    // -- Setup & Fixtures
    let dir = TempDir::new()?;
    write_cwd_persona(
        dir.path(),
        "bad-perms",
        "---\nname: Bad Perms\npermissions:\n  \"*\": nonsense\n---\nBody",
    )?;
    let mut state = PersonaState::new(None, None, None, None, None, None);

    // -- Exec
    let err = switch(&mut state, dir.path(), "bad-perms")
        .err()
        .ok_or("should reject invalid persona permissions")?;

    // -- Check
    assert!(
        err.starts_with("agent switch failed: invalid permissions: invalid permission action"),
        "err={err}"
    );
    assert_eq!(state.agent_identity(), Some("Bad Perms"));
    assert_eq!(
        state.agent_persona_body().map(str::trim),
        Some("Body"),
        "the body and identity land before the overlay is parsed"
    );
    Ok(())
}
