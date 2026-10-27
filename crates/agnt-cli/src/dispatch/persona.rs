//! clap-based persona resolution for the CLI.

use agnt_core::config::AgentsConfig;
use agnt_core::protocol::persona::{
    FrontMatterParser, FsPersonaResolver, ParsedPersona, PersonaFileResolver,
    PulldownCmarkFrontMatterParser, interpret_front_matter,
};
use agnt_core::tools::authz::PermissionsOverlay;
use nu_protocol::{LabeledError, Span};

use crate::error::{Error, Result};

// region:    --- Types

/// Persona and identity resolved from clap flags and config.
pub struct PersonaResolution {
    pub persona: Option<ParsedPersona>,
    /// The persona slug: the `--agent` value (or the configured default) when a
    /// persona loaded, `None` otherwise. This is the value hook envelopes carry
    /// as `agent`; it is not the front-matter display name.
    pub agent_slug: Option<String>,
    pub agent_identity: Option<String>,
    pub messaging_identity: Option<String>,
    pub agent_permissions_overlay: Option<PermissionsOverlay>,
}

// endregion: --- Types

// region:    --- Functions

/// Resolve the effective persona, identities, and permissions overlay.
///
/// Effective agent name precedence:
/// 1. `--agent` flag (validated against disabled built-ins)
/// 2. `agents.default` / `agents.fallback` from config.toml
pub fn resolve_persona_from_clap(
    agent_name: Option<String>,
    cli_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &std::path::Path,
) -> Result<PersonaResolution> {
    let span = Span::unknown();

    let effective_agent_name = if let Some(name) = agent_name {
        if agnt_core::protocol::persona::builtins::is_builtin_persona(&name)
            && !agents_config.is_builtin_enabled(&name)
        {
            return Err(Error::Runtime(
                LabeledError::new(format!(
                    "Agent '{}' is disabled in config. Enable it or use a different agent.",
                    name
                ))
                .with_label("disabled agent", span),
            ));
        }
        Some(name)
    } else {
        agents_config
            .resolve_default_agent()
            .map_err(|msg| Error::Runtime(LabeledError::new(msg)))?
    };
    log::debug!("effective_agent_name={effective_agent_name:?}");

    let persona = if let Some(name) = &effective_agent_name {
        let cwd = std::path::PathBuf::from(cwd);
        let config_dir = agnt_core::utils::xdg::config_dir()
            .map(|base| base.join("nu-agent"))
            .map_err(|e| {
                Error::Runtime(LabeledError::new(format!(
                    "Cannot determine config directory: {e}"
                )))
            })?;

        let resolver = FsPersonaResolver::new(cwd, config_dir, agents_config.clone());
        let (_path, contents) = resolver.resolve(name).map_err(|e| {
            Error::Runtime(LabeledError::new(format!("Agent persona not found: {e}")))
        })?;

        let parser = PulldownCmarkFrontMatterParser;
        let raw = parser.parse(&contents).map_err(|e| {
            Error::Runtime(LabeledError::new(format!(
                "Invalid agent persona front matter: {e}"
            )))
        })?;

        Some(
            interpret_front_matter(raw.front_matter.as_ref(), raw.body).map_err(|e| {
                Error::Runtime(LabeledError::new(format!(
                    "Invalid agent persona front matter: {e}"
                )))
            })?,
        )
    } else {
        None
    };

    // Display identity: persona name > effective agent name (never --name)
    let agent_identity = persona
        .as_ref()
        .and_then(|p| p.name.clone())
        .or_else(|| effective_agent_name.clone());
    // Persona slug: the effective agent name, but only when a persona loaded.
    let agent_slug = persona.as_ref().and(effective_agent_name.clone());
    // Messaging identity: --name > display identity (for multi-agent communication)
    let messaging_identity = cli_name.or_else(|| agent_identity.clone());

    let agent_permissions_overlay = persona
        .as_ref()
        .and_then(|p| p.permissions.as_ref())
        .map(PermissionsOverlay::parse_from_yaml)
        .transpose()
        .map_err(|msg| {
            Error::Runtime(LabeledError::new(format!(
                "Invalid agent permissions: {msg}"
            )))
        })?;

    Ok(PersonaResolution {
        persona,
        agent_slug,
        agent_identity,
        messaging_identity,
        agent_permissions_overlay,
    })
}

// endregion: --- Functions

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/dispatch/persona.rs"]
mod persona_test;

// endregion: --- Tests
