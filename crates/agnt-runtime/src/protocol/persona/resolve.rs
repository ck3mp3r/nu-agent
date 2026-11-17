//! Shared persona resolution seam.
//!
//! One implementation of every resolution step: effective agent name, persona
//! load, front-matter interpretation, identities, and permissions overlay. It is
//! a pure transform over its parameters and mutates no config.

use std::path::Path;

use nu_protocol::{LabeledError, Span};

use crate::config::AgentsConfig;
use crate::tools::authz::PermissionsOverlay;

use super::builtins;
use super::error::PersonaLoadError;
use super::parser::{
    FrontMatterParser, ParsedPersona, PulldownCmarkFrontMatterParser, interpret_front_matter,
};
use super::resolver::{FsPersonaResolver, PersonaFileResolver};

// region:    --- Types

/// Persona and identity resolved from an agent name and the config.
pub struct PersonaResolution {
    pub persona: Option<ParsedPersona>,
    /// The persona slug: the requested agent name (or the configured default)
    /// when a persona loaded, `None` otherwise. This is the value hook
    /// envelopes carry as `agent`; it is not the front-matter display name.
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
/// 1. `agent_name` (validated against disabled built-ins)
/// 2. `agents.default` / `agents.fallback` from config
pub fn resolve_persona(
    agent_name: Option<String>,
    cli_name: Option<String>,
    agents_config: &AgentsConfig,
    cwd: &Path,
    agents_dir: &Path,
    span: Span,
) -> Result<PersonaResolution, LabeledError> {
    let effective_agent_name = if let Some(name) = agent_name {
        if builtins::is_builtin_persona(&name) && !agents_config.is_builtin_enabled(&name) {
            return Err(LabeledError::new(format!(
                "Agent '{}' is disabled in config. Enable it or use a different agent.",
                name
            ))
            .with_label("disabled agent", span));
        }
        Some(name)
    } else {
        agents_config
            .resolve_default_agent()
            .map_err(LabeledError::new)?
    };

    let persona = if let Some(name) = &effective_agent_name {
        Some(
            load_persona(name, cwd, agents_dir, agents_config).map_err(|error| {
                let (message, detail) = match error {
                    PersonaLoadError::Load(inner) => (
                        format!("Agent persona not found: {inner}"),
                        inner.to_string(),
                    ),
                    PersonaLoadError::Parse(inner) | PersonaLoadError::Interpret(inner) => (
                        format!("Invalid agent persona front matter: {inner}"),
                        inner.to_string(),
                    ),
                };
                LabeledError::new(message).with_label(detail, span)
            })?,
        )
    } else {
        None
    };

    // Display identity: persona name > effective agent name (never `cli_name`)
    let agent_identity = persona
        .as_ref()
        .and_then(|p| p.name.clone())
        .or_else(|| effective_agent_name.clone());
    // Persona slug: the effective agent name, but only when a persona loaded.
    let agent_slug = persona.as_ref().and(effective_agent_name.clone());
    // Messaging identity: `cli_name` > display identity
    let messaging_identity = cli_name.or_else(|| agent_identity.clone());

    let agent_permissions_overlay = match persona.as_ref() {
        Some(persona) => persona.permissions_overlay().map_err(|message| {
            LabeledError::new(format!("Invalid agent permissions: {message}"))
                .with_label(message, span)
        })?,
        None => None,
    };

    Ok(PersonaResolution {
        persona,
        agent_slug,
        agent_identity,
        messaging_identity,
        agent_permissions_overlay,
    })
}

/// Load, parse, and interpret one persona file.
///
/// The failure keeps its leg, so a caller composes the prefix it reports over
/// that leg. Name precedence and the disabled-built-in check are the caller's:
/// this reads the one name it is given.
pub(crate) fn load_persona(
    name: &str,
    cwd: &Path,
    agents_dir: &Path,
    agents_config: &AgentsConfig,
) -> Result<ParsedPersona, PersonaLoadError> {
    let resolver = FsPersonaResolver::new(
        cwd.to_path_buf(),
        agents_dir.to_path_buf(),
        agents_config.clone(),
    );
    let (_path, contents) = resolver.resolve(name).map_err(PersonaLoadError::Load)?;

    let parser = PulldownCmarkFrontMatterParser;
    let raw = parser.parse(&contents).map_err(PersonaLoadError::Parse)?;

    interpret_front_matter(raw.front_matter.as_ref(), raw.body).map_err(PersonaLoadError::Interpret)
}

// endregion: --- Functions
