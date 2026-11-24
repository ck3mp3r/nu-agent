//! `resolve_model_spec_for`: which provider the planning spec is filed under.
//!
//! The wire owns the name its facts and spec are filed under, so the wire name
//! is the primary input. The config's own spelling is consumed only in the
//! window before a build, and only through the spellings the Copilot client
//! already accepts.

use super::*;

use crate::config::Config;
use crate::conversation::compaction::compactor::resolve_model_spec_for;
use rig::providers::registry::{Format, ProviderId};

/// A config that spells its provider `provider`, as the loader leaves a
/// `[providers.<name>]` block that declares no rig impl of its own.
fn config_for(provider: &str) -> Config {
    Config {
        provider: provider.to_string(),
        model: "gpt-4o".to_string(),
        ..Config::default()
    }
}

/// The Copilot wire name: what `bind_and_erase` reads from
/// `model.wire.describe().name` for a Copilot model.
fn wire_copilot_name() -> &'static str {
    rig::providers::copilot::DIALECT.name
}

/// The id rig registers for the Copilot wire name.
fn wire_copilot_id() -> Result<ProviderId> {
    let name = wire_copilot_name();
    ProviderId::new(name, Format::OpenAi)
        .ok_or_else(|| format!("rig must register its own Copilot wire name '{name}'").into())
}

/// WHEN the wire name is not available yet, THE fallback SHALL resolve each
/// Copilot spelling to the one id rig registers.
#[test]
fn test_conversation_compaction_vendor_fallback_resolves_the_copilot_spellings() -> Result<()> {
    // -- Setup & Fixtures
    let expected = wire_copilot_id()?;

    // -- Exec & Check
    for spelling in ["copilot", "github-copilot", "github_copilot"] {
        let config = config_for(spelling);
        let spec = resolve_model_spec_for(&config, None, None)
            .ok_or_else(|| format!("'{spelling}' must resolve to a rig provider id"))?;
        assert_eq!(
            spec.provider, expected,
            "'{spelling}' must be planned under the id rig registers for Copilot"
        );
    }
    Ok(())
}

/// THE fallback's id for a Copilot spelling SHALL be the id the wire reports.
///
/// A rig rename of the Copilot dialect breaks this equality, so the fallback
/// cannot rot into a silent `None` behind a name rig no longer registers.
#[test]
fn test_conversation_compaction_vendor_fallback_matches_the_wire_name() -> Result<()> {
    // -- Setup & Fixtures
    let wire_name = wire_copilot_name();
    let wire_id = wire_copilot_id()?;
    let config = config_for("github-copilot");

    // -- Exec
    let spec =
        resolve_model_spec_for(&config, None, None).ok_or("the Copilot spelling must resolve")?;

    // -- Check
    assert_eq!(
        spec.provider, wire_id,
        "the fallback must plan under the wire name '{wire_name}': a rig rename must fail here, \
         not disable compaction silently"
    );
    Ok(())
}

/// WHEN the wire has reported its own name, THEN the resolver SHALL take it
/// before the config's spelling: the wire owns the name its spec is filed under.
#[test]
fn test_conversation_compaction_vendor_wire_name_wins_over_the_config_spelling() -> Result<()> {
    // -- Setup & Fixtures: the config spells Copilot, and the wire the build
    // landed on reports Ollama.
    let config = config_for("github-copilot");
    let wire_name = rig::providers::ollama::PROVIDER_NAME;
    let wire_id = ProviderId::new(wire_name, Format::OpenAi)
        .ok_or_else(|| format!("rig must register the Ollama wire name '{wire_name}'"))?;

    // -- Exec
    let spec = resolve_model_spec_for(&config, None, Some(wire_name))
        .ok_or("a wire name rig registers must resolve the spec")?;

    // -- Check
    assert_eq!(
        spec.provider, wire_id,
        "the wire name must win over the config spelling"
    );
    Ok(())
}

/// WHEN no accepted spelling names the provider, THEN the resolver SHALL keep
/// the disable rather than plan under an arbitrary registered provider.
#[test]
fn test_conversation_compaction_vendor_unknown_spelling_stays_disabled() -> Result<()> {
    // -- Setup & Fixtures
    let config = config_for("no-such-vendor");

    // -- Exec
    let spec = resolve_model_spec_for(&config, None, None);

    // -- Check
    assert!(
        spec.is_none(),
        "a spelling no table accepts must resolve to no provider id, got {spec:?}"
    );
    Ok(())
}
